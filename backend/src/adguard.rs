use reqwest::header::{HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::path::Path;
use std::time::{Duration, Instant};
use crate::config::AdGuardConfig;

/// Быстрое и безопасное кодирование в Base64 без внешних зависимостей
pub fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((input.len() + 2) / 3 * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = if chunk.len() > 1 { chunk[1] as usize } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as usize } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[(n >> 18) & 63] as char);
        out.push(TABLE[(n >> 12) & 63] as char);
        if chunk.len() > 1 {
            out.push(TABLE[(n >> 6) & 63] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[n & 63] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// Форматирование заголовка HTTP Basic Auth для API AdGuard Home
pub fn build_auth_header(username: Option<&str>, password: Option<&str>) -> Option<String> {
    let u = username.unwrap_or("").trim();
    let p = password.unwrap_or("").trim();
    if u.is_empty() && p.is_empty() {
        return None;
    }
    let creds = format!("{}:{}", u, p);
    Some(format!("Basic {}", base64_encode(creds.as_bytes())))
}

/// Базовый URL для обращения к API AdGuard Home
pub fn agh_base_url(cfg: &AdGuardConfig) -> String {
    format!("http://{}:{}", cfg.host, cfg.http_port)
}

/// Универсальный HTTP-клиент для вызовов AdGuard Home REST API
pub async fn agh_request(
    http: &reqwest::Client,
    cfg: &AdGuardConfig,
    method: reqwest::Method,
    path: &str,
    body: Option<Value>,
    timeout_sec: u64,
) -> Result<Value, String> {
    let url = format!("{}{}", agh_base_url(cfg), path);
    let mut req = http
        .request(method, &url)
        .header(CONTENT_TYPE, "application/json")
        .timeout(Duration::from_secs(timeout_sec));

    if let Some(auth_val) = build_auth_header(cfg.username.as_deref(), cfg.password.as_deref()) {
        if let Ok(val) = HeaderValue::from_str(&auth_val) {
            req = req.header(AUTHORIZATION, val);
        }
    }

    if let Some(b) = body {
        req = req.json(&b);
    }

    let resp = req.send().await.map_err(|e| {
        if e.is_connect() {
            format!(
                "AdGuard Home недоступен по адресу {}:{} (служба остановлена или порт закрыт)",
                cfg.host, cfg.http_port
            )
        } else if e.is_timeout() {
            format!("Таймаут запроса к AdGuard Home ({timeout_sec}с)")
        } else {
            format!("Ошибка соединения с AdGuard Home: {e}")
        }
    })?;

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err("Ошибка авторизации AdGuard Home: неверный логин или пароль".to_string());
    }

    if !status.is_success() {
        let err_text = resp.text().await.unwrap_or_default();
        return Err(format!("AdGuard Home вернул ошибку HTTP {status}: {err_text}"));
    }

    if status == reqwest::StatusCode::NO_CONTENT {
        return Ok(Value::Null);
    }

    let text = resp.text().await.map_err(|e| format!("Не удалось прочитать ответ AdGuard Home: {e}"))?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(Value::Null);
    }

    serde_json::from_str::<Value>(trimmed).or_else(|_| Ok(json!({ "raw": trimmed })))
}

/// Статус работы AdGuard Home
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AghStatus {
    pub running: bool,
    pub version: String,
    pub protection_enabled: bool,
    pub dns_addresses: Vec<String>,
    pub dns_port: u16,
    pub http_port: u16,
    pub language: String,
    pub start_time: Option<u64>,
}

/// Запрос статуса (/control/status)
pub async fn get_status(http: &reqwest::Client, cfg: &AdGuardConfig) -> AghStatus {
    match agh_request(http, cfg, reqwest::Method::GET, "/control/status", None, 3).await {
        Ok(v) => {
            let version = v.get("version").and_then(|s| s.as_str()).unwrap_or("").to_string();
            let protection_enabled = v
                .get("protection_enabled")
                .or_else(|| v.get("enabled"))
                .and_then(|b| b.as_bool())
                .unwrap_or(false);
            let dns_port = v.get("dns_port").and_then(|p| p.as_u64()).map(|p| p as u16).unwrap_or(cfg.dns_port);
            let http_port = v.get("http_port").and_then(|p| p.as_u64()).map(|p| p as u16).unwrap_or(cfg.http_port);
            let language = v.get("language").and_then(|s| s.as_str()).unwrap_or("ru").to_string();
            let start_time = v.get("start_time").and_then(|t| t.as_u64());

            let mut dns_addresses = Vec::new();
            if let Some(arr) = v.get("dns_addresses").and_then(|a| a.as_array()) {
                for item in arr {
                    if let Some(s) = item.as_str() {
                        dns_addresses.push(s.to_string());
                    }
                }
            }

            AghStatus {
                running: true,
                version,
                protection_enabled,
                dns_addresses,
                dns_port,
                http_port,
                language,
                start_time,
            }
        }
        Err(_) => AghStatus {
            running: false,
            version: String::new(),
            protection_enabled: false,
            dns_addresses: Vec::new(),
            dns_port: cfg.dns_port,
            http_port: cfg.http_port,
            language: "ru".to_string(),
            start_time: None,
        },
    }
}

/// Поддерживаемые возможности AdGuard Home
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AghCapabilities {
    pub protection_control: bool,
    pub query_log: bool,
    pub filtering: bool,
    pub custom_rules: bool,
    pub rewrites: bool,
    pub clients: bool,
    pub stats: bool,
    pub managed_service: bool,
}

pub fn get_capabilities(cfg: &AdGuardConfig) -> AghCapabilities {
    AghCapabilities {
        protection_control: true,
        query_log: true,
        filtering: true,
        custom_rules: true,
        rewrites: true,
        clients: true,
        stats: true,
        managed_service: cfg.integration_mode == "managed",
    }
}

/// Проверка риска циклической пересылки DNS (Loop Risk)
pub fn detect_agh_loop_risk(
    upstreams: &[String],
    router_ip: Option<&str>,
    local_dns_port: u16,
) -> (bool, Option<String>) {
    let local_port_str = format!(":{}", local_dns_port);
    let r_ip = router_ip.unwrap_or("192.168.1.1").trim();

    for upstream in upstreams {
        let u = upstream.trim();
        if u.is_empty() || u.starts_with('#') {
            continue;
        }

        // Проверка loopback
        if u.contains("127.0.0.1:53")
            || u.contains("localhost:53")
            || u.contains("127.0.0.1:1053")
            || u.contains("localhost:1053")
            || u.contains("127.0.0.1:7874")
        {
            return (
                true,
                Some(format!(
                    "Upstream DNS '{u}' указывает на локальный порт роутера, что вызывает критический DNS-цикл!"
                )),
            );
        }

        // Проверка IP роутера на стандартных портах
        if !r_ip.is_empty()
            && (u.starts_with(r_ip) || u.contains(&format!("//{r_ip}")))
            && (u.ends_with(":53") || u.ends_with(&local_port_str) || !u.contains(':'))
        {
            return (
                true,
                Some(format!(
                    "Upstream DNS '{u}' указывает на IP роутера ({r_ip}), что вызывает зацикливание запросов!"
                )),
            );
        }
    }

    (false, None)
}

/// Результат полной проверки здоровья AdGuard Home
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AghHealth {
    pub process_alive: bool,
    pub api_ok: bool,
    pub api_latency_ms: u64,
    pub dns_listener_ok: bool,
    pub loop_risk: bool,
    pub loop_warning: Option<String>,
    pub version: Option<String>,
    pub protection_enabled: bool,
}

/// Комплексный аудит работоспособности AdGuard Home
pub async fn get_health(
    http: &reqwest::Client,
    cfg: &AdGuardConfig,
    router_ip: Option<&str>,
) -> AghHealth {
    // 1. Проверка процесса
    let process_alive = {
        #[cfg(target_os = "linux")]
        {
            crate::system::is_process_running("AdGuardHome")
        }
        #[cfg(not(target_os = "linux"))]
        {
            true
        }
    };

    // 2. Проверка HTTP API и замер задержки
    let start = Instant::now();
    let status_res = agh_request(http, cfg, reqwest::Method::GET, "/control/status", None, 3).await;
    let api_latency_ms = start.elapsed().as_millis() as u64;
    let (api_ok, version, protection_enabled) = match status_res {
        Ok(v) => (
            true,
            v.get("version").and_then(|s| s.as_str()).map(|s| s.to_string()),
            v.get("protection_enabled").or_else(|| v.get("enabled")).and_then(|b| b.as_bool()).unwrap_or(false),
        ),
        Err(_) => (false, None, false),
    };

    // 3. Проверка DNS-прослушивателя через реальный UDP-запрос
    let dns_listener_ok = test_agh_dns_listener(&cfg.host, cfg.dns_port).await;

    // 4. Проверка риска циклов
    let mut loop_risk = false;
    let mut loop_warning = None;

    if api_ok {
        if let Ok(dns_info) = agh_request(http, cfg, reqwest::Method::GET, "/control/dns_info", None, 3).await {
            let mut upstreams = Vec::new();
            if let Some(arr) = dns_info.get("upstream_dns").and_then(|a| a.as_array()) {
                for item in arr {
                    if let Some(s) = item.as_str() {
                        upstreams.push(s.to_string());
                    }
                }
            }
            let (lr, lw) = detect_agh_loop_risk(&upstreams, router_ip, 1053);
            loop_risk = lr;
            loop_warning = lw;
        }
    }

    AghHealth {
        process_alive,
        api_ok,
        api_latency_ms,
        dns_listener_ok,
        loop_risk,
        loop_warning,
        version,
        protection_enabled,
    }
}

/// Тестирование локального UDP DNS слушателя AdGuard Home
pub async fn test_agh_dns_listener(host: &str, port: u16) -> bool {
    let addr = match format!("{}:{}", host, port).parse::<SocketAddr>() {
        Ok(a) => a,
        Err(_) => return false,
    };

    let bind_addr = if addr.is_ipv6() { "[::]:0" } else { "0.0.0.0:0" };
    let sock = match tokio::net::UdpSocket::bind(bind_addr).await {
        Ok(s) => s,
        Err(_) => return false,
    };

    if sock.connect(addr).await.is_err() {
        return false;
    }

    // DNS-запрос для probe.test (IN A)
    let query: [u8; 28] = [
        0xAD, 0x64, // TxID
        0x01, 0x00, // Flags: standard query, RD=1
        0x00, 0x01, // QDCOUNT: 1
        0x00, 0x00, // ANCOUNT: 0
        0x00, 0x00, // NSCOUNT: 0
        0x00, 0x00, // ARCOUNT: 0
        0x05, b'p', b'r', b'o', b'b', b'e',
        0x04, b't', b'e', b's', b't',
        0x00,       // null label
        0x00, 0x01, // QTYPE: A
        0x00, 0x01, // QCLASS: IN
    ];

    if sock.send(&query).await.is_err() {
        return false;
    }

    let mut buf = [0u8; 512];
    match tokio::time::timeout(Duration::from_millis(1500), sock.recv(&mut buf)).await {
        Ok(Ok(n)) => crate::zapret::validate_dns_response((0xAD, 0x64), &buf[..n]),
        _ => false,
    }
}

/// Запись статистики домена
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainStat {
    pub domain: String,
    pub count: u64,
}

/// Запись статистики клиента
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientStat {
    pub ip: String,
    pub name: Option<String>,
    pub count: u64,
}

/// Агрегированные данные дашборда AdGuard Home
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AghOverview {
    pub running: bool,
    pub protection_enabled: bool,
    pub num_dns_queries: u64,
    pub num_blocked_filtering: u64,
    pub num_replaced_safebrowsing: u64,
    pub num_replaced_parental: u64,
    pub num_replaced_safesearch: u64,
    pub avg_processing_time_ms: f64,
    pub block_percentage: f64,
    pub active_rules_count: u64,
    pub top_queried_domains: Vec<DomainStat>,
    pub top_blocked_domains: Vec<DomainStat>,
    pub top_clients: Vec<ClientStat>,
}

/// Получение сводного дашборда AdGuard Home
pub async fn get_overview(http: &reqwest::Client, cfg: &AdGuardConfig) -> Result<AghOverview, String> {
    let status = get_status(http, cfg).await;
    if !status.running {
        return Ok(AghOverview {
            running: false,
            protection_enabled: false,
            num_dns_queries: 0,
            num_blocked_filtering: 0,
            num_replaced_safebrowsing: 0,
            num_replaced_parental: 0,
            num_replaced_safesearch: 0,
            avg_processing_time_ms: 0.0,
            block_percentage: 0.0,
            active_rules_count: 0,
            top_queried_domains: Vec::new(),
            top_blocked_domains: Vec::new(),
            top_clients: Vec::new(),
        });
    }

    let stats_val = agh_request(http, cfg, reqwest::Method::GET, "/control/stats?time_unit=hours&time_units=24", None, 4)
        .await
        .unwrap_or(Value::Null);

    let filtering_val = agh_request(http, cfg, reqwest::Method::GET, "/control/filtering/status", None, 4)
        .await
        .unwrap_or(Value::Null);

    let num_dns_queries = stats_val.get("num_dns_queries").and_then(|v| v.as_u64()).unwrap_or(0);
    let num_blocked_filtering = stats_val.get("num_blocked_filtering").and_then(|v| v.as_u64()).unwrap_or(0);
    let num_replaced_safebrowsing = stats_val.get("num_replaced_safebrowsing").and_then(|v| v.as_u64()).unwrap_or(0);
    let num_replaced_parental = stats_val.get("num_replaced_parental").and_then(|v| v.as_u64()).unwrap_or(0);
    let num_replaced_safesearch = stats_val.get("num_replaced_safesearch").and_then(|v| v.as_u64()).unwrap_or(0);
    let avg_processing_time_ms = stats_val.get("avg_processing_time").and_then(|v| v.as_f64()).unwrap_or(0.0) * 1000.0;

    let block_percentage = if num_dns_queries > 0 {
        ((num_blocked_filtering as f64 / num_dns_queries as f64) * 1000.0).round() / 10.0
    } else {
        0.0
    };

    // Подсчет правил
    let mut active_rules_count = 0u64;
    if let Some(filters) = filtering_val.get("filters").and_then(|f| f.as_array()) {
        for flt in filters {
            if flt.get("enabled").and_then(|b| b.as_bool()).unwrap_or(false) {
                active_rules_count += flt.get("rules_count").and_then(|c| c.as_u64()).unwrap_or(0);
            }
        }
    }
    if let Some(user_rules) = filtering_val.get("user_rules").and_then(|u| u.as_array()) {
        active_rules_count += user_rules.len() as u64;
    }

    // Парсинг топов
    let mut top_queried_domains = Vec::new();
    if let Some(arr) = stats_val.get("top_queried_domains").and_then(|a| a.as_array()) {
        for item in arr.iter().take(10) {
            if let Some(obj) = item.as_object() {
                for (k, v) in obj {
                    top_queried_domains.push(DomainStat {
                        domain: k.clone(),
                        count: v.as_u64().unwrap_or(0),
                    });
                }
            }
        }
    }

    let mut top_blocked_domains = Vec::new();
    if let Some(arr) = stats_val.get("top_blocked_domains").and_then(|a| a.as_array()) {
        for item in arr.iter().take(10) {
            if let Some(obj) = item.as_object() {
                for (k, v) in obj {
                    top_blocked_domains.push(DomainStat {
                        domain: k.clone(),
                        count: v.as_u64().unwrap_or(0),
                    });
                }
            }
        }
    }

    let mut top_clients = Vec::new();
    if let Some(arr) = stats_val.get("top_clients").and_then(|a| a.as_array()) {
        for item in arr.iter().take(10) {
            if let Some(obj) = item.as_object() {
                for (k, v) in obj {
                    let count = if let Some(n) = v.as_u64() {
                        n
                    } else if let Some(sub) = v.as_object() {
                        sub.get("count").and_then(|c| c.as_u64()).unwrap_or(0)
                    } else {
                        0
                    };
                    top_clients.push(ClientStat {
                        ip: k.clone(),
                        name: None,
                        count,
                    });
                }
            }
        }
    }

    Ok(AghOverview {
        running: true,
        protection_enabled: status.protection_enabled,
        num_dns_queries,
        num_blocked_filtering,
        num_replaced_safebrowsing,
        num_replaced_parental,
        num_replaced_safesearch,
        avg_processing_time_ms,
        block_percentage,
        active_rules_count,
        top_queried_domains,
        top_blocked_domains,
        top_clients,
    })
}

/// Элемент журнала DNS-запросов (Query Log)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryLogItem {
    pub timestamp: String,
    pub client_ip: String,
    pub client_name: Option<String>,
    pub question_name: String,
    pub question_type: String,
    pub status: String,
    pub elapsed_ms: f64,
    pub reason: String,
    pub rule: Option<String>,
    pub filter_id: Option<i64>,
    pub answer: Vec<String>,
}

/// Ответ журнала запросов
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryLogResponse {
    pub data: Vec<QueryLogItem>,
    pub oldest: Option<String>,
}

/// Получение журнала DNS-запросов с фильтрацией и пагинацией
pub async fn get_query_log(
    http: &reqwest::Client,
    cfg: &AdGuardConfig,
    limit: usize,
    older_than: Option<&str>,
    search: Option<&str>,
    response_status: Option<&str>,
) -> Result<QueryLogResponse, String> {
    let mut params = vec![format!("limit={}", limit.clamp(1, 200))];
    if let Some(ot) = older_than {
        if !ot.is_empty() {
            params.push(format!("older_than={}", urlencoding_lite(ot)));
        }
    }
    if let Some(s) = search {
        if !s.is_empty() {
            params.push(format!("search={}", urlencoding_lite(s)));
        }
    }
    if let Some(rs) = response_status {
        if !rs.is_empty() {
            params.push(format!("response_status={}", urlencoding_lite(rs)));
        }
    }

    let path = format!("/control/querylog?{}", params.join("&"));
    let res = agh_request(http, cfg, reqwest::Method::GET, &path, None, 5).await?;

    let mut items = Vec::new();
    let oldest = res.get("oldest").and_then(|s| s.as_str()).map(|s| s.to_string());

    if let Some(arr) = res.get("data").and_then(|d| d.as_array()) {
        for row in arr {
            let timestamp = row.get("time").and_then(|t| t.as_str()).unwrap_or("").to_string();
            let client_ip = row.get("client").and_then(|c| c.as_str()).unwrap_or("").to_string();
            let client_name = row
                .get("client_info")
                .and_then(|ci| ci.get("name"))
                .and_then(|n| n.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());

            let q = row.get("question");
            let question_name = q.and_then(|q| q.get("name")).and_then(|n| n.as_str()).unwrap_or("").to_string();
            let question_type = q.and_then(|q| q.get("type")).and_then(|t| t.as_str()).unwrap_or("A").to_string();

            let status = row.get("status").and_then(|s| s.as_str()).unwrap_or("OK").to_string();
            let elapsed_ms = row.get("elapsed_ms").and_then(|e| e.as_f64()).unwrap_or(0.0);
            let reason = row.get("reason").and_then(|r| r.as_str()).unwrap_or("").to_string();
            let rule = row.get("rule").and_then(|r| r.as_str()).map(|r| r.to_string());
            let filter_id = row.get("filter_id").and_then(|f| f.as_i64());

            let mut answer = Vec::new();
            if let Some(ans_arr) = row.get("answer").and_then(|a| a.as_array()) {
                for ans in ans_arr {
                    if let Some(text) = ans.get("text").and_then(|t| t.as_str()) {
                        answer.push(text.to_string());
                    } else if let Some(val) = ans.as_str() {
                        answer.push(val.to_string());
                    }
                }
            }

            items.push(QueryLogItem {
                timestamp,
                client_ip,
                client_name,
                question_name,
                question_type,
                status,
                elapsed_ms,
                reason,
                rule,
                filter_id,
                answer,
            });
        }
    }

    Ok(QueryLogResponse { data: items, oldest })
}

/// Простое URL-кодирование для query параметров
fn urlencoding_lite(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.as_bytes() {
        match *b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(*b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// Включение/выключение защиты AdGuard Home с обязательной пост-валидацией
pub async fn set_protection(http: &reqwest::Client, cfg: &AdGuardConfig, enabled: bool) -> Result<bool, String> {
    let body = json!({
        "protection_enabled": enabled,
        "enabled": enabled
    });

    agh_request(http, cfg, reqwest::Method::POST, "/control/protection", Some(body), 5).await?;

    // Пост-валидация: убеждаемся, что статус действительно изменился
    let status = get_status(http, cfg).await;
    if !status.running {
        return Err("AdGuard Home перестал отвечать после изменения статуса защиты".to_string());
    }
    if status.protection_enabled != enabled {
        return Err(format!(
            "Не удалось применить статус защиты (запрошено: {}, текущее: {})",
            enabled, status.protection_enabled
        ));
    }

    Ok(status.protection_enabled)
}

/// Фильтр подписки AdGuard Home
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterSubscription {
    pub id: i64,
    pub name: String,
    pub url: String,
    pub rules_count: u64,
    pub enabled: bool,
    pub last_updated: Option<String>,
}

/// Полный статус фильтрации
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AghFilteringStatus {
    pub enabled: bool,
    pub interval: u32,
    pub filters: Vec<FilterSubscription>,
    pub whitelist_filters: Vec<FilterSubscription>,
    pub user_rules: Vec<String>,
}

/// Получение состояния фильтрации и списков правил
pub async fn get_filtering(http: &reqwest::Client, cfg: &AdGuardConfig) -> Result<AghFilteringStatus, String> {
    let val = agh_request(http, cfg, reqwest::Method::GET, "/control/filtering/status", None, 5).await?;

    let enabled = val.get("enabled").and_then(|b| b.as_bool()).unwrap_or(false);
    let interval = val.get("interval").and_then(|i| i.as_u64()).map(|i| i as u32).unwrap_or(24);

    let parse_filters = |key: &str| -> Vec<FilterSubscription> {
        let mut out = Vec::new();
        if let Some(arr) = val.get(key).and_then(|a| a.as_array()) {
            for f in arr {
                let id = f.get("id").and_then(|i| i.as_i64()).unwrap_or(0);
                let name = f.get("name").and_then(|s| s.as_str()).unwrap_or("").to_string();
                let url = f.get("url").and_then(|s| s.as_str()).unwrap_or("").to_string();
                let rules_count = f.get("rules_count").and_then(|c| c.as_u64()).unwrap_or(0);
                let enabled = f.get("enabled").and_then(|b| b.as_bool()).unwrap_or(false);
                let last_updated = f.get("last_updated").and_then(|s| s.as_str()).map(|s| s.to_string());

                out.push(FilterSubscription {
                    id,
                    name,
                    url,
                    rules_count,
                    enabled,
                    last_updated,
                });
            }
        }
        out
    };

    let filters = parse_filters("filters");
    let whitelist_filters = parse_filters("whitelist_filters");

    let mut user_rules = Vec::new();
    if let Some(rules) = val.get("user_rules").and_then(|r| r.as_array()) {
        for r in rules {
            if let Some(s) = r.as_str() {
                user_rules.push(s.to_string());
            }
        }
    }

    Ok(AghFilteringStatus {
        enabled,
        interval,
        filters,
        whitelist_filters,
        user_rules,
    })
}

/// Валидация синтаксиса пользовательских правил AdGuard Home
pub fn validate_user_rules(rules: &[String]) -> Result<(), String> {
    for (idx, line) in rules.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('!') || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.len() > 2048 {
            return Err(format!(
                "Строка {}: превышена максимальная длина правила (макс 2048 симв.)",
                idx + 1
            ));
        }
        // Защита от случайного блокирования всего интернета пустыми масками
        if trimmed == "*" || trimmed == "||*" || trimmed == "|*" {
            return Err(format!(
                "Строка {}: опасное правило '{}' заблокирует весь трафик сети!",
                idx + 1, trimmed
            ));
        }
    }
    Ok(())
}

/// Сохранение пользовательских правил AdGuard Home
pub async fn set_user_rules(http: &reqwest::Client, cfg: &AdGuardConfig, rules: &[String]) -> Result<(), String> {
    validate_user_rules(rules)?;
    let body = json!({ "rules": rules });
    agh_request(http, cfg, reqwest::Method::POST, "/control/filtering/set_rules", Some(body), 5).await?;
    Ok(())
}

/// Переключение активности конкретного списка фильтрации
pub async fn toggle_filter(
    http: &reqwest::Client,
    cfg: &AdGuardConfig,
    url: &str,
    enabled: bool,
    whitelist: bool,
) -> Result<(), String> {
    let body = json!({
        "url": url,
        "whitelist": whitelist,
        "data": {
            "enabled": enabled,
            "url": url,
        }
    });
    agh_request(http, cfg, reqwest::Method::POST, "/control/filtering/set_url", Some(body), 5).await?;
    Ok(())
}

/// Запись DNS Rewrite (переопределения домена)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewriteEntry {
    pub domain: String,
    pub answer: String,
}

/// Валидация домена для DNS Rewrite (включая локальные зоны .lan, .local и wildcard)
pub fn is_valid_rewrite_domain(domain: &str) -> bool {
    let d = domain.trim();
    if d.is_empty() || d.len() > 253 {
        return false;
    }
    let check = d.strip_prefix("*.").unwrap_or(d);
    if check.is_empty() {
        return false;
    }
    for label in check.split('.') {
        if label.is_empty() || label.len() > 63 {
            return false;
        }
        if label.starts_with('-') || label.ends_with('-') {
            return false;
        }
        if !label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return false;
        }
    }
    true
}

/// Валидация DNS Rewrite
pub fn validate_rewrite(domain: &str, answer: &str) -> Result<(), String> {
    let d = domain.trim();
    let a = answer.trim();

    if d.is_empty() {
        return Err("Домен не может быть пустым".to_string());
    }

    if !is_valid_rewrite_domain(d) {
        return Err(format!("Некорректный формат домена: '{d}'"));
    }

    if a.is_empty() {
        return Err("Ответ DNS не может быть пустым".to_string());
    }

    let is_ip = a.parse::<std::net::IpAddr>().is_ok();
    let is_cname = is_valid_rewrite_domain(a);
    let is_special = a == "A" || a == "AAAA";

    if !is_ip && !is_cname && !is_special {
        return Err(format!("Ответ DNS должен быть IP-адресом или корректным доменом: '{a}'"));
    }

    Ok(())
}

/// Получение списка переопределений DNS
pub async fn get_rewrites(http: &reqwest::Client, cfg: &AdGuardConfig) -> Result<Vec<RewriteEntry>, String> {
    let val = agh_request(http, cfg, reqwest::Method::GET, "/control/rewrite/list", None, 4).await?;
    let mut out = Vec::new();
    if let Some(arr) = val.as_array() {
        for item in arr {
            if let (Some(d), Some(a)) = (
                item.get("domain").and_then(|s| s.as_str()),
                item.get("answer").and_then(|s| s.as_str()),
            ) {
                out.push(RewriteEntry {
                    domain: d.to_string(),
                    answer: a.to_string(),
                });
            }
        }
    }
    Ok(out)
}

/// Добавление DNS Rewrite
pub async fn add_rewrite(
    http: &reqwest::Client,
    cfg: &AdGuardConfig,
    domain: &str,
    answer: &str,
) -> Result<(), String> {
    validate_rewrite(domain, answer)?;
    let body = json!({
        "domain": domain.trim(),
        "answer": answer.trim(),
    });
    agh_request(http, cfg, reqwest::Method::POST, "/control/rewrite/add", Some(body), 5).await?;
    Ok(())
}

/// Удаление DNS Rewrite
pub async fn delete_rewrite(
    http: &reqwest::Client,
    cfg: &AdGuardConfig,
    domain: &str,
    answer: &str,
) -> Result<(), String> {
    let body = json!({
        "domain": domain.trim(),
        "answer": answer.trim(),
    });
    agh_request(http, cfg, reqwest::Method::POST, "/control/rewrite/delete", Some(body), 5).await?;
    Ok(())
}

/// Клиент AdGuard Home
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AghClient {
    pub name: String,
    pub ids: Vec<String>,
    pub use_global_settings: bool,
    pub filtering_enabled: bool,
    pub parental_enabled: bool,
    pub safebrowsing_enabled: bool,
    pub blocked_services: Vec<String>,
    pub upstreams: Vec<String>,
}

/// Получение списка настроенных клиентов
pub async fn get_clients(http: &reqwest::Client, cfg: &AdGuardConfig) -> Result<Vec<AghClient>, String> {
    let val = agh_request(http, cfg, reqwest::Method::GET, "/control/clients", None, 5).await?;
    let mut out = Vec::new();

    if let Some(clients_arr) = val.get("clients").and_then(|c| c.as_array()) {
        for c in clients_arr {
            let name = c.get("name").and_then(|s| s.as_str()).unwrap_or("").to_string();
            let mut ids = Vec::new();
            if let Some(ids_arr) = c.get("ids").and_then(|i| i.as_array()) {
                for id in ids_arr {
                    if let Some(s) = id.as_str() {
                        ids.push(s.to_string());
                    }
                }
            }

            let use_global_settings = c.get("use_global_settings").and_then(|b| b.as_bool()).unwrap_or(true);
            let filtering_enabled = c.get("filtering_enabled").and_then(|b| b.as_bool()).unwrap_or(true);
            let parental_enabled = c.get("parental_enabled").and_then(|b| b.as_bool()).unwrap_or(false);
            let safebrowsing_enabled = c.get("safebrowsing_enabled").and_then(|b| b.as_bool()).unwrap_or(false);

            let mut blocked_services = Vec::new();
            if let Some(bs_arr) = c.get("blocked_services").and_then(|b| b.as_array()) {
                for bs in bs_arr {
                    if let Some(s) = bs.as_str() {
                        blocked_services.push(s.to_string());
                    }
                }
            }

            let mut upstreams = Vec::new();
            if let Some(up_arr) = c.get("upstreams").and_then(|u| u.as_array()) {
                for up in up_arr {
                    if let Some(s) = up.as_str() {
                        upstreams.push(s.to_string());
                    }
                }
            }

            out.push(AghClient {
                name,
                ids,
                use_global_settings,
                filtering_enabled,
                parental_enabled,
                safebrowsing_enabled,
                blocked_services,
                upstreams,
            });
        }
    }

    Ok(out)
}

/// Диагностика системного окружения AdGuard Home
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AghDiagnostics {
    pub port_53_status: String,
    pub port_3000_status: String,
    pub dnsmasq_redirect_active: bool,
    pub iptables_redirect_active: bool,
    pub detected_service_path: Option<String>,
    pub detected_config_path: Option<String>,
    pub loop_risk: bool,
    pub recommendations: Vec<String>,
}

/// Запуск комплексной диагностики интеграции AdGuard Home
pub async fn get_diagnostics(
    http: &reqwest::Client,
    cfg: &AdGuardConfig,
    router_ip: Option<&str>,
) -> AghDiagnostics {
    let mut recommendations = Vec::new();

    // 1. Проверка доступности портов
    let port_53_active = test_agh_dns_listener(&cfg.host, cfg.dns_port).await;
    let port_53_status = if port_53_active {
        "active".to_string()
    } else {
        "inactive".to_string()
    };

    let status = get_status(http, cfg).await;
    let port_3000_status = if status.running {
        "active".to_string()
    } else {
        "inactive".to_string()
    };

    // 2. Обнаружение файлов конфигурации и службы
    let service_paths = [
        "/opt/etc/init.d/S99adguardhome",
        "/opt/etc/init.d/S99AdGuardHome",
    ];
    let detected_service_path = service_paths.iter().find(|p| Path::new(p).exists()).map(|s| s.to_string());

    let config_paths = [
        "/opt/etc/AdGuardHome.yaml",
        "/opt/etc/AdGuardHome/AdGuardHome.yaml",
        "AdGuardHome.yaml",
    ];
    let detected_config_path = config_paths.iter().find(|p| Path::new(p).exists()).map(|s| s.to_string());

    // 3. Проверка iptables NAT редиректов (Linux)
    #[allow(unused_mut)]
    let mut iptables_redirect_active = false;
    #[cfg(target_os = "linux")]
    {
        if let Ok(out) = std::process::Command::new("iptables")
            .args(["-t", "nat", "-S"])
            .output()
        {
            let text = String::from_utf8_lossy(&out.stdout);
            iptables_redirect_active = text.contains("--dport 53 -j REDIRECT") || text.contains("REDIRECT --to-ports 53");
        }
    }

    let dnsmasq_redirect_active = iptables_redirect_active;

    // 4. Проверка риска циклов
    let mut loop_risk = false;
    if status.running {
        if let Ok(dns_info) = agh_request(http, cfg, reqwest::Method::GET, "/control/dns_info", None, 3).await {
            let mut upstreams = Vec::new();
            if let Some(arr) = dns_info.get("upstream_dns").and_then(|a| a.as_array()) {
                for item in arr {
                    if let Some(s) = item.as_str() {
                        upstreams.push(s.to_string());
                    }
                }
            }
            let (lr, lw) = detect_agh_loop_risk(&upstreams, router_ip, 1053);
            loop_risk = lr;
            if let Some(w) = lw {
                recommendations.push(w);
            }
        }
    }

    // Рекомендации
    if !status.running {
        recommendations.push("Служба AdGuard Home остановлена или веб-интерфейс недоступен на порту 3000.".to_string());
    } else if !port_53_active {
        recommendations.push("AdGuard Home запущен, но UDP-порт DNS 53 не отвечает на запросы.".to_string());
    }

    if status.running && !status.protection_enabled {
        recommendations.push("Защита AdGuard Home временно выключена — блокировка рекламы и трекеров не выполняется.".to_string());
    }

    if detected_service_path.is_none() && cfg.integration_mode == "managed" {
        recommendations.push("Init-скрипт службы (/opt/etc/init.d/S99adguardhome) не найден. Для работы требуется пакет adguardhome из Entware.".to_string());
    }

    AghDiagnostics {
        port_53_status,
        port_3000_status,
        dnsmasq_redirect_active,
        iptables_redirect_active,
        detected_service_path,
        detected_config_path,
        loop_risk,
        recommendations,
    }
}

/// Управление системной службой AdGuard Home (/opt/etc/init.d/S99adguardhome)
pub async fn service_action(action: &str) -> Result<String, String> {
    let act = action.trim().to_lowercase();
    if !["start", "stop", "restart", "status"].contains(&act.as_str()) {
        return Err(format!("Недопустимое действие службы: '{action}'"));
    }

    #[cfg(target_os = "linux")]
    {
        let script = if Path::new("/opt/etc/init.d/S99adguardhome").exists() {
            "/opt/etc/init.d/S99adguardhome"
        } else if Path::new("/opt/etc/init.d/S99AdGuardHome").exists() {
            "/opt/etc/init.d/S99AdGuardHome"
        } else {
            return Err("Служба AdGuard Home не найдена в /opt/etc/init.d/".to_string());
        };

        let out = tokio::process::Command::new(script)
            .arg(&act)
            .output()
            .await
            .map_err(|e| format!("Ошибка выполнения {script} {act}: {e}"))?;

        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        if !out.status.success() && act != "status" {
            return Err(format!("Ошибка команды {act}: {stderr} {stdout}"));
        }
        Ok(if stdout.is_empty() { stderr } else { stdout })
    }

    #[cfg(not(target_os = "linux"))]
    {
        Ok(format!("[dev/win] Выполнена команда службы AdGuard Home: {act}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adguard_auth_header_format() {
        assert_eq!(build_auth_header(None, None), None);
        assert_eq!(build_auth_header(Some(""), Some("")), None);
        assert_eq!(
            build_auth_header(Some("admin"), Some("secret123")),
            Some("Basic YWRtaW46c2VjcmV0MTIz".to_string())
        );
    }

    #[test]
    fn test_adguard_loop_risk_detection() {
        let upstreams_safe = vec![
            "tls://1.1.1.1".to_string(),
            "https://dns.google/dns-query".to_string(),
        ];
        let (risk, warn) = detect_agh_loop_risk(&upstreams_safe, Some("192.168.1.1"), 1053);
        assert!(!risk);
        assert!(warn.is_none());

        let upstreams_loop_local = vec![
            "127.0.0.1:53".to_string(),
            "tls://1.1.1.1".to_string(),
        ];
        let (risk, warn) = detect_agh_loop_risk(&upstreams_loop_local, Some("192.168.1.1"), 1053);
        assert!(risk);
        assert!(warn.unwrap().contains("локальный порт"));

        let upstreams_loop_router = vec![
            "192.168.1.1:53".to_string(),
        ];
        let (risk, warn) = detect_agh_loop_risk(&upstreams_loop_router, Some("192.168.1.1"), 1053);
        assert!(risk);
        assert!(warn.unwrap().contains("зацикливание"));
    }

    #[test]
    fn test_adguard_filter_rules_validation() {
        let valid_rules = vec![
            "||example.com^".to_string(),
            "! Это комментарий".to_string(),
            "@@||safe.com^".to_string(),
        ];
        assert!(validate_user_rules(&valid_rules).is_ok());

        let dangerous_rules = vec![
            "||*".to_string(),
        ];
        assert!(validate_user_rules(&dangerous_rules).is_err());

        let wild_rule = vec![
            "*".to_string(),
        ];
        assert!(validate_user_rules(&wild_rule).is_err());
    }

    #[test]
    fn test_adguard_rewrite_validation() {
        assert!(validate_rewrite("example.com", "192.168.1.100").is_ok());
        assert!(validate_rewrite("*.corp.lan", "10.0.0.1").is_ok());
        assert!(validate_rewrite("proxy.local", "A").is_ok());
        assert!(validate_rewrite("sub.domain.com", "target.com").is_ok());

        // Невалидный домен
        assert!(validate_rewrite("invalid..domain", "1.2.3.4").is_err());
        // Невалидный ответ
        assert!(validate_rewrite("test.com", "not a valid answer or ip!").is_err());
    }

    #[test]
    fn test_adguard_capabilities_managed_mode() {
        let mut cfg = AdGuardConfig::default();
        cfg.integration_mode = "managed".to_string();
        let cap = get_capabilities(&cfg);
        assert!(cap.managed_service);
        assert!(cap.protection_control);
        assert!(cap.query_log);

        cfg.integration_mode = "external".to_string();
        let cap_ext = get_capabilities(&cfg);
        assert!(!cap_ext.managed_service);
    }
}
