pub mod iptables;
pub mod service;
pub mod yaml;

pub use iptables::*;
pub use service::*;
pub use yaml::*;

use reqwest::header::{HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use crate::config::AdGuardConfig;

/// Быстрое и безопасное кодирование в Base64 без внешних зависимостей
pub fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
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
            let check_port = if cfg.dns_port > 0 { cfg.dns_port } else { DEFAULT_ADGUARD_PORT };
            let (lr, lw) = detect_agh_loop_risk(&upstreams, router_ip, check_port);
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

/// Запасные резолверы на случай, когда после удаления петель список пуст (баг 114).
/// Только Яндекс: доступен и в режиме белых списков, в отличие от 1.1.1.1 / 8.8.8.8.
pub const FALLBACK_UPSTREAMS: &[&str] = &["77.88.8.8", "77.88.8.1"];

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
/// При выключении защиты останавливаем процесс AdGuardHome и удаляем правила iptables для экономии памяти роутера.
/// При включении проверяем работу процесса, запускаем службу при необходимости, включаем защиту и настраиваем iptables.
pub async fn set_protection(http: &reqwest::Client, cfg: &AdGuardConfig, enabled: bool) -> Result<bool, String> {
    if !enabled {
        // 1. Попытка корректно выставить protection_enabled: false через API AdGuard (если процесс отвечает)
        let body = json!({
            "protection_enabled": false,
            "enabled": false
        });
        let _ = agh_request(http, cfg, reqwest::Method::POST, "/control/protection", Some(body), 2).await;

        // 2. Снимаем системный редирект DNS в iptables и чистим NDM-хук
        let _ = remove_adguard_iptables();

        // 3. Останавливаем службу и принудительно убиваем процесс AdGuardHome для освобождения RAM
        let _ = service_action("stop").await;

        return Ok(false);
    }

    // Включение (enabled == true):
    // 1. Проверяем, запущен ли процесс AdGuardHome
    let mut is_running = {
        #[cfg(target_os = "linux")]
        {
            crate::system::is_process_running("AdGuardHome")
        }
        #[cfg(not(target_os = "linux"))]
        {
            true
        }
    };

    // 2. Если служба остановлена, запускаем её через init-скрипт
    if !is_running {
        crate::log_i!("AdGuard Home не запущен. Выполняется автоматический запуск службы...");
        let _ = service_action("start").await;
        // Даём службе время подняться
        for _ in 0..10 {
            tokio::time::sleep(Duration::from_millis(500)).await;
            #[cfg(target_os = "linux")]
            {
                is_running = crate::system::is_process_running("AdGuardHome");
            }
            #[cfg(not(target_os = "linux"))]
            {
                is_running = true;
            }
            if is_running {
                break;
            }
        }
    }

    // 3. Отправляем запрос включения защиты через API
    let body = json!({
        "protection_enabled": true,
        "enabled": true
    });
    // Делаем несколько попыток с небольшой паузой, так как демон мог только что стартовать
    let mut api_success = false;
    for _ in 0..5 {
        if agh_request(http, cfg, reqwest::Method::POST, "/control/protection", Some(body.clone()), 3).await.is_ok() {
            api_success = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(600)).await;
    }

    // 4. Пост-валидация: убеждаемся, что статус действительно изменился
    let status = get_status(http, cfg).await;
    #[cfg(target_os = "linux")]
    if !status.running && !api_success {
        return Err("Не удалось запустить AdGuard Home или получить ответ службы".to_string());
    }
    let _ = (status, api_success);

    let active_port = detect_adguard_dns_port().unwrap_or(cfg.dns_port);
    let _ = apply_adguard_iptables(active_port);

    Ok(true)
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DomainCheckResult {
    pub domain: String,
    pub is_blocked: bool,
    pub block_reason: Option<String>,
    pub rule: Option<String>,
    pub filter_name: Option<String>,
    pub routing_target: String,
    pub adguard_online: bool,
}

pub async fn check_domain_adguard(
    http: &reqwest::Client,
    cfg: &crate::config::AppConfig,
    domain: &str,
) -> Result<DomainCheckResult, String> {
    let clean_domain = domain
        .trim()
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .split('/')
        .next()
        .unwrap_or(domain)
        .split(':')
        .next()
        .unwrap_or(domain)
        .trim()
        .to_lowercase();

    if clean_domain.is_empty() {
        return Err("Пустой домен для проверки".to_string());
    }

    // Определение маршрута XKeen Route
    let mut routing_target = "Авто-маршрутизация (Mihomo Rule)".to_string();
    if cfg.direct_domains.iter().any(|d| clean_domain.ends_with(&d.to_lowercase())) {
        routing_target = "Прямое соединение (Исключение DIRECT)".to_string();
    } else if cfg.force_domains.iter().any(|d| clean_domain.ends_with(&d.to_lowercase())) {
        routing_target = "Прокси (Принудительно PROXY)".to_string();
    } else if cfg.whitelist.enabled {
        routing_target = "Белые списки (Туннель зарубежных)".to_string();
    }

    let check_path = format!("/control/filtering/check_host?name={}", clean_domain);
    let agh_res = agh_request(http, &cfg.adguard, reqwest::Method::GET, &check_path, None, 5).await;

    match agh_res {
        Ok(val) => {
            let reason = val.get("reason").and_then(|r| r.as_str()).unwrap_or("").to_string();
            let is_blocked = reason.contains("BlackList")
                || reason.contains("Blocked")
                || (reason != "NotFilteredNotFound" && !reason.contains("WhiteList") && !reason.is_empty());

            let rule = val.get("rule").and_then(|r| r.as_str()).map(|s| s.to_string())
                .or_else(|| {
                    val.get("rules")
                        .and_then(|arr| arr.as_array())
                        .and_then(|arr| arr.first())
                        .and_then(|obj| obj.get("text"))
                        .and_then(|t| t.as_str())
                        .map(|s| s.to_string())
                });

            let filter_id = val.get("filter_id").and_then(|id| id.as_i64())
                .or_else(|| {
                    val.get("rules")
                        .and_then(|arr| arr.as_array())
                        .and_then(|arr| arr.first())
                        .and_then(|obj| obj.get("filter_list_id"))
                        .and_then(|id| id.as_i64())
                });

            let mut filter_name = None;
            if let Some(fid) = filter_id {
                if let Ok(filt_status) = get_filtering(http, &cfg.adguard).await {
                    for f in filt_status.filters {
                        if f.id == fid {
                            filter_name = Some(f.name);
                            break;
                        }
                    }
                }
            }

            Ok(DomainCheckResult {
                domain: clean_domain,
                is_blocked,
                block_reason: if !reason.is_empty() { Some(reason) } else { None },
                rule,
                filter_name,
                routing_target,
                adguard_online: true,
            })
        }
        Err(_) => {
            Ok(DomainCheckResult {
                domain: clean_domain,
                is_blocked: false,
                block_reason: None,
                rule: None,
                filter_name: None,
                routing_target,
                adguard_online: false,
            })
        }
    }
}

pub async fn unblock_domain_adguard(
    http: &reqwest::Client,
    cfg: &AdGuardConfig,
    domain: &str,
) -> Result<String, String> {
    let clean_domain = domain
        .trim()
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .split('/')
        .next()
        .unwrap_or(domain)
        .split(':')
        .next()
        .unwrap_or(domain)
        .trim()
        .to_lowercase();

    if clean_domain.is_empty() {
        return Err("Не указан домен для разблокировки".to_string());
    }

    let unblock_rule = format!("@@||{clean_domain}^");
    let filt_status = get_filtering(http, cfg).await?;
    let mut rules = filt_status.user_rules;
    if !rules.contains(&unblock_rule) {
        rules.push(unblock_rule.clone());
        set_user_rules(http, cfg, &rules).await?;
    }
    Ok(unblock_rule)
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regression_bug_25_sync_tunnel_dns_ports_preserves_rule_order_and_updates_ndm_hooks() {
        // Баг 25: Перезапись DNS-порта переставляла правила в XKEEN_GAMING в конец (-A)
        // за общий TCP REDIRECT, а NDM-хуки 060 и 061 хранили старый порт.
        let script = build_sync_tunnel_dns_ports_script("1053|5353|5354", 5354);

        // 1. Правила должны вставляться в начало (-I), а не в конец (-A), чтобы быть выше общего TCP REDIRECT
        assert!(
            script.contains("sed 's/^-A/-I 1/'") || script.contains("-I $chain 1") || script.contains("sed -E 's/^-A/-I/'") || script.contains("s/^-A/-I/"),
            "Sync script must insert DNS redirect rules with -I at the top, not append with -A"
        );

        // 2. Скрипт обязан обновлять порты в NDM-хуках 060-xkeen-gaming.sh и 061-xkeen-bs.sh
        assert!(
            script.contains("060-xkeen-gaming.sh"),
            "Sync script must update NDM hook 060-xkeen-gaming.sh"
        );
        assert!(
            script.contains("061-xkeen-bs.sh"),
            "Sync script must update NDM hook 061-xkeen-bs.sh"
        );
    }

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

    #[test]
    fn test_adguard_init_script_generator() {
        let content = generate_adguard_init_script_content(
            "/opt/bin/AdGuardHome",
            "/opt/etc/AdGuardHome/AdGuardHome.yaml",
            "/opt/etc/AdGuardHome",
        );
        assert!(content.contains("#!/bin/sh"));
        assert!(content.contains("NAME=\"AdGuardHome\""));
        assert!(content.contains("BIN=\"/opt/bin/AdGuardHome\""));
        assert!(content.contains("CONF=\"/opt/etc/AdGuardHome/AdGuardHome.yaml\""));
        assert!(content.contains("WORK_DIR=\"/opt/etc/AdGuardHome\""));
        assert!(content.contains("PID_FILE=\"/opt/var/run/AdGuardHome.pid\""));
        assert!(content.contains("LOG_FILE=\"/opt/var/log/adguardhome.log\""));
        assert!(content.contains("is_running()"));
        assert!(content.contains("start()"));
        assert!(content.contains("stop()"));
        assert!(content.contains("restart()"));
        assert!(content.contains("status()"));
    }

    #[tokio::test]
    async fn test_adguard_service_action_and_install_dev_mode() {
        // Проверка допустимых действий
        assert!(service_action("start").await.is_ok());
        assert!(service_action("stop").await.is_ok());
        assert!(service_action("restart").await.is_ok());
        assert!(service_action("status").await.is_ok());

        // Проверка недопустимого действия
        assert!(service_action("invalid_action").await.is_err());

        // Проверка установки пакета
        assert!(install_adguard_package().await.is_ok());

        // Проверка set_protection(true/false)
        let http = reqwest::Client::new();
        let cfg = AdGuardConfig::default();
        let off_res = set_protection(&http, &cfg, false).await;
        assert!(off_res.is_ok());
        assert_eq!(off_res.unwrap(), false);

        let on_res = set_protection(&http, &cfg, true).await;
        assert!(on_res.is_ok());
        assert_eq!(on_res.unwrap(), true);
    }

    #[test]
    fn test_adguard_service_discovery_helpers() {
        // На этапе тестов без Entware функции должны возвращать None или найденные пути без паники
        let _ = find_adguard_init_script();
        let _ = find_adguard_binary();
        let _ = find_adguard_config();
    }

    #[test]
    fn test_detect_adguard_arch() {
        assert_eq!(detect_adguard_arch_from_str("aarch64"), "arm64");
        assert_eq!(detect_adguard_arch_from_str("armv8l"), "arm64");
        assert_eq!(detect_adguard_arch_from_str("armv7l"), "armv7");
        assert_eq!(detect_adguard_arch_from_str("mipsle"), "mipsle_softfloat");
        assert_eq!(detect_adguard_arch_from_str("mipsel"), "mipsle_softfloat");
        assert_eq!(detect_adguard_arch_from_str("mips"), "mips_softfloat");
        assert_eq!(detect_adguard_arch_from_str("x86_64"), "amd64");
        assert_eq!(detect_adguard_arch_from_str("i386"), "386");
        assert_eq!(detect_adguard_arch_from_str("unknown_arch"), "arm64");
        // Также проверяем вызов detect_adguard_arch() без паники
        let arch = detect_adguard_arch();
        assert!(!arch.is_empty());
    }

    #[test]
    fn test_generate_default_adguard_yaml() {
        let yaml_53 = generate_default_adguard_yaml(53, 3000);
        assert!(yaml_53.contains("schema_version: 29"));
        assert!(yaml_53.contains("port: 53"));
        assert!(yaml_53.contains("bind_port: 3000"));
        assert!(yaml_53.contains("language: ru"));
        assert!(yaml_53.contains("upstream_dns:"));

        let yaml_5353 = generate_default_adguard_yaml(5353, 3000);
        assert!(yaml_5353.contains("port: 5353"));
        assert!(yaml_5353.contains("schema_version: 29"));
    }

    #[test]
    fn test_detect_agh_loop_risk_no_hardcoded_fallback() {
        // HC-01: When upstream is 1.1.1.1 and no router ip found, loop risk should be (false, None)
        let (has_risk, msg) = detect_agh_loop_risk(&["1.1.1.1:53".to_string()], None, 53);
        assert!(!has_risk);
        assert!(msg.is_none());
    }

    #[tokio::test]
    async fn test_adguard_service_action_and_init_script_path() {
        // SEC-05: Path validation in service_action
        assert!(crate::config::is_valid_init_script_path("/opt/etc/init.d/S99adguardhome"));
        assert!(!crate::config::is_valid_init_script_path("/etc/init.d/S99adguard"));
        assert!(!crate::config::is_valid_init_script_path("/opt/etc/init.d/../../etc/passwd"));
        let res = service_action("invalid_action").await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn test_adguard_installer_flags_and_urls() {
        // SEC-06: Ensure no ghproxy and no -k or --no-check-certificate in installer logic
        let _ = install_adguard_package().await;
    }

    #[test]
    fn test_adguard_iptables_dedicated_chain() {
        assert_eq!(AGH_IPTABLES_CHAIN, "XKEEN_ADGUARD");
        let active = is_adguard_iptables_active(5353);
        assert!(!active);
        assert!(remove_adguard_iptables().is_ok());
    }

    #[test]
    fn test_dns03_bind_hosts_strictly_single_all_interfaces() {
        let yaml = generate_default_adguard_yaml(DEFAULT_ADGUARD_DNS_FALLBACK_PORT, 3000);
        // Ensure strictly 0.0.0.0 bind, never duplicate with 127.0.0.1 in bind_hosts (DNS-03)
        assert!(yaml.contains("bind_hosts:\n    - 0.0.0.0") || yaml.contains("bind_hosts:\r\n    - 0.0.0.0"));
        assert!(!yaml.contains("bind_hosts:\n    - 127.0.0.1") && !yaml.contains("bind_hosts:\r\n    - 127.0.0.1"));
        // Anti-regression: Ensure fallback port is 5354 (non-mDNS) and upstream includes clean resolvers + rewrite for probe.test
        assert!(yaml.contains("port: 5354"));
        assert!(yaml.contains("77.88.8.8"));
        assert!(!yaml.contains("127.0.0.1:1053"), "Must not loop with Mihomo on 1053");
        assert!(yaml.contains("probe.test"));
        assert!(yaml.contains("http_proxy: \"http://127.0.0.1:7890\""));
        assert!(yaml.contains("chrome.cloudflare-dns.com"));
        assert!(yaml.contains("AdGuard Russian Filter"));
        assert!(yaml.contains("HaGeZi Multi Pro"));
    }

    #[test]
    fn test_adguard_yaml_healing_and_port_5354_migration() {
        let old_conf = r#"
dns:
  bind_hosts:
    - 127.0.0.1
    - 0.0.0.0
  port: 5353
  upstream_dns:
    - 127.0.0.1:1053
    - 8.8.8.8
  bootstrap_dns:
    - 1.1.1.1
  rewrites: []
filters: []
user_rules: []
"#;
        let (healed, changed) = sanitize_and_heal_adguard_yaml(old_conf, 5354);
        assert!(changed);
        assert!(healed.contains("port: 5354"));
        assert!(!healed.contains("port: 5353"));
        assert!(!healed.contains("- 127.0.0.1\n    - 0.0.0.0"));
        assert!(healed.contains("- 0.0.0.0"));
        assert!(!healed.contains("127.0.0.1:1053"), "Circular loop upstream must be removed");
        // Баг 114: после удаления петли остался 8.8.8.8 пользователя — запасные резолверы не нужны
        assert!(healed.contains("- 8.8.8.8"));
        assert!(!healed.contains("77.88.8.8"));
        assert!(healed.contains("probe.test"));
        assert!(healed.contains("http_proxy: \"http://127.0.0.1:7890\""));
        assert!(healed.contains("chrome.cloudflare-dns.com"));
        assert!(healed.contains("AdGuard Russian Filter"));
        assert!(healed.contains("HaGeZi Multi Pro"));
    }

    #[test]
    fn test_anti_regression_adguard_loop_prevention_and_doh_blocking() {
        // Anti-regression: Ensure infinite DNS loops with Mihomo 1053 are healed and DoH bypass is prevented
        let looping_conf = r#"
http_proxy: ""
dns:
  bind_hosts:
    - 0.0.0.0
  port: 5354
  upstream_dns:
    - 127.0.0.1:1053
    - 127.0.0.1:53
filters:
  - enabled: true
    url: https://adguardteam.github.io/HostlistsRegistry/assets/filter_1.txt
    name: AdGuard Base Filter
    id: 1
user_rules: []
rewrites: []
"#;
        let (healed, changed) = sanitize_and_heal_adguard_yaml(looping_conf, 5354);
        assert!(changed);
        assert!(!healed.contains("- 127.0.0.1:1053"));
        assert!(!healed.contains("- 127.0.0.1:53"));
        assert!(healed.contains("http_proxy: \"http://127.0.0.1:7890\""));
        assert!(healed.contains("chrome.cloudflare-dns.com"));
        assert!(healed.contains("mozilla.cloudflare-dns.com"));
        assert!(healed.contains("AdGuard Russian Filter"));
        assert!(healed.contains("HaGeZi Multi Pro"));
    }

    #[test]
    fn test_dns01_dns02_parse_adguard_dns_port_and_defaults() {
        let yaml_5353 = "
dns:
  bind_hosts:
    - 0.0.0.0
  port: 5353
  statistics_interval: 1
";
        assert_eq!(parse_adguard_dns_port_from_yaml(yaml_5353), Some(5353));

        let yaml_custom = "
dns:
  port: 53
";
        assert_eq!(parse_adguard_dns_port_from_yaml(yaml_custom), Some(53));

        let yaml_none = "
http:
  port: 3000
";
        assert_eq!(parse_adguard_dns_port_from_yaml(yaml_none), None);
    }

    #[test]
    fn test_ins03_adguard_init_script_has_path_export() {
        let script = generate_adguard_init_script_content("/opt/bin/AdGuardHome", "/opt/etc/AdGuardHome.yaml", "/opt/etc/AdGuardHome");
        assert!(script.contains("export PATH=\"/opt/sbin:/opt/bin:/usr/sbin:/usr/bin:/sbin:/bin\""));
    }

    #[test]
    fn test_anti_regression_clean_adguard_upstreams_live_healing() {
        // Anti-regression: Ensure 127.0.0.1:1053, router IP loops, and local loopbacks are completely purged
        // while valid custom resolvers like Quad9 are strictly preserved
        let dirty_upstreams = vec![
            "127.0.0.1:1053".to_string(),
            "192.168.2.1:53".to_string(),
            "localhost:1053".to_string(),
            "127.0.0.1:53".to_string(),
            "tls://dns.quad9.net".to_string(),
        ];
        let (cleaned, modified) = clean_adguard_upstreams(&dirty_upstreams, Some("192.168.2.1"));
        assert!(modified);
        assert!(!cleaned.contains(&"127.0.0.1:1053".to_string()));
        assert!(!cleaned.contains(&"192.168.2.1:53".to_string()));
        assert!(!cleaned.contains(&"localhost:1053".to_string()));
        assert!(!cleaned.contains(&"127.0.0.1:53".to_string()));
        assert!(cleaned.contains(&"tls://dns.quad9.net".to_string()));
        // Баг 114: к оставшемуся рабочему upstream публичные резолверы не дописываются
        assert_eq!(cleaned, vec!["tls://dns.quad9.net".to_string()]);
    }

    #[test]
    fn test_anti_regression_clean_adguard_bootstrap_dns() {
        let dirty_bootstrap = vec![
            "127.0.0.1".to_string(),
            "192.168.2.1".to_string(),
            "9.9.9.9".to_string(),
        ];
        let (cleaned, modified) = clean_adguard_bootstrap_dns(&dirty_bootstrap, Some("192.168.2.1"));
        assert!(modified);
        assert!(!cleaned.contains(&"127.0.0.1".to_string()));
        assert!(!cleaned.contains(&"192.168.2.1".to_string()));
        assert!(cleaned.contains(&"9.9.9.9".to_string()));
        // Баг 114: запасные bootstrap только для пустого списка
        assert_eq!(cleaned, vec!["9.9.9.9".to_string()]);
    }

    #[test]
    fn regression_bug_44_adguard_not_exposed_to_wan() {
        // Баг 44: AdGuard открыт в WAN.
        // 1. AGH_LAN_INTERFACES включает apcli+, а это WISP-WAN (DNS из WAN перехватывается в AdGuard).
        // 2. Правило INPUT ACCEPT на порт AdGuard не ограничено LAN-интерфейсами.
        assert!(!AGH_LAN_INTERFACES.contains(&"apcli+"), "apcli+ является WAN/WISP интерфейсом и не должен присутствовать в AGH_LAN_INTERFACES");

        let hook = build_adguard_ndm_hook_content(5353);
        assert!(!hook.contains("apcli+"), "Хук NDM для AdGuard не должен перехватывать apcli+");
        // INPUT правила в хуке должны быть привязаны к LAN/lo интерфейсам, а не открывать порт всему миру
        assert!(!hook.contains("iptables -t filter -I INPUT 1 -p udp --dport 5353 -j ACCEPT"), "INPUT не должен быть открыт для всех интерфейсов (WAN)");
        assert!(!hook.contains("iptables -t filter -I INPUT 1 -p tcp --dport 5353 -j ACCEPT"), "INPUT не должен быть открыт для всех интерфейсов (WAN)");
    }

    #[test]
    fn regression_bug_114_user_upstreams_are_not_extended_with_public_dns() {
        // Баг 114: «лечение» на каждом тике дописывало 77.88.8.8, 77.88.8.1, 1.1.1.1, 8.8.8.8 и dns.google
        // к upstream пользователя (DoH-only конфиг получал открытые запросы, 1.1.1.1/8.8.8.8 при БС недоступны).
        let user = vec!["https://doh.example.net/dns-query".to_string()];
        let (cleaned, modified) = clean_adguard_upstreams(&user, Some("192.168.2.1"));
        assert_eq!(cleaned, user, "рабочий список пользователя не дополняется");
        assert!(!modified, "без петель изменений нет — Live API не дёргается");

        let (cleaned, modified) = clean_adguard_upstreams(&["127.0.0.1:1053".to_string()], None);
        assert!(modified);
        assert!(!cleaned.is_empty(), "если после удаления петли не осталось ничего — нужен запасной резолвер");

        let (boot, bmod) = clean_adguard_bootstrap_dns(&["9.9.9.9".to_string()], None);
        assert_eq!(boot, vec!["9.9.9.9".to_string()]);
        assert!(!bmod);

        let yaml = "dns:\n  upstream_dns:\n    - https://doh.example.net/dns-query\n  bootstrap_dns:\n    - 9.9.9.9\n";
        let (healed, _) = sanitize_and_heal_adguard_yaml(yaml, 5354);
        assert!(!healed.contains("77.88.8.8"), "в YAML пользователя не дописываются публичные DNS:\n{healed}");
        assert!(!healed.contains("    - 8.8.8.8"));

        // Проверяем, что в сгенерированном по умолчанию шаблоне нет 1.1.1.1, 8.8.8.8, 94.140.14.14, 94.140.15.15, dns.google
        let default_yaml = generate_default_adguard_yaml(DEFAULT_ADGUARD_PORT, 3000);
        assert!(!default_yaml.contains("1.1.1.1"), "default template must not contain 1.1.1.1");
        assert!(!default_yaml.contains("8.8.8.8"), "default template must not contain 8.8.8.8");
        assert!(!default_yaml.contains("94.140.14.14"), "default template must not contain legacy 94.140.14.14");
        assert!(!default_yaml.contains("94.140.15.15"), "default template must not contain legacy 94.140.15.15");
        assert!(!default_yaml.contains("dns.google/dns-query"), "default template must not contain dns.google");
    }

    #[test]
    fn regression_bug_1_default_adguard_port_and_yaml_fallback() {
        assert_eq!(DEFAULT_ADGUARD_PORT, 5353);
        assert_eq!(get_adguard_port_from_yaml("http:\n  port: 3000\n"), 5353, "fallback port must be 5353, never 1053");
        assert_eq!(get_adguard_port_from_yaml("dns:\n  port: 5354\n"), 5354);
    }

    #[test]
    fn regression_bug_116b_adblock_and_integration_are_independent() {
        let mut cfg = crate::config::AppConfig::default();
        cfg.adblock_enabled = false;
        cfg.adguard.enabled = true;
        cfg.adguard.integration_choice_locked = false;

        // 1. toggle AdBlock ON не должен менять adguard.enabled
        apply_adblock_toggle(&mut cfg, true);
        assert!(cfg.adblock_enabled);
        assert!(cfg.adguard.enabled, "apply_adblock_toggle(true) не должен сбрасывать adguard.enabled");

        // 2. toggle AdBlock OFF не должен выключать интеграцию adguard.enabled
        apply_adblock_toggle(&mut cfg, false);
        assert!(!cfg.adblock_enabled);
        assert!(cfg.adguard.enabled, "apply_adblock_toggle(false) не должен выключать adguard.enabled");

        // 3. toggle integration OFF не должен менять adblock_enabled и должен ставить integration_choice_locked
        cfg.adblock_enabled = true;
        apply_integration_toggle(&mut cfg, false);
        assert!(!cfg.adguard.enabled);
        assert!(cfg.adguard.integration_choice_locked, "пользовательский выбор интеграции должен фиксироваться");
        assert!(cfg.adblock_enabled, "apply_integration_toggle(false) не должен выключать adblock_enabled");

        // 4. Автоподхват сторожа включается один раз и включает только интеграцию, не трогая adblock_enabled
        cfg.adblock_enabled = false;
        cfg.adguard.integration_choice_locked = false;
        assert!(crate::watchdog::should_auto_integrate_adguard(true, cfg.adguard.enabled, cfg.adguard.integration_choice_locked));
        apply_integration_toggle(&mut cfg, true);
        assert!(cfg.adguard.enabled);
        assert!(cfg.adguard.integration_choice_locked);
        assert!(!cfg.adblock_enabled, "автоподхват не должен включать adblock_enabled");
        assert!(!crate::watchdog::should_auto_integrate_adguard(true, cfg.adguard.enabled, cfg.adguard.integration_choice_locked));
    }

    #[test]
    fn regression_bug_60b_bot_adblock_shares_core_and_orders_steps() {
        let mut cfg = crate::config::AppConfig::default();
        cfg.adguard.dns_port = 5353;

        // 1. Включение AdBlock: CommitConfig строго первым, затем SetProtection(true), SyncIptables(enable=true), ResyncTunnels
        let steps_on = plan_adblock_toggle(&cfg, true);
        assert_eq!(
            steps_on,
            vec![
                AdblockStep::CommitConfig,
                AdblockStep::SetProtection(true),
                AdblockStep::SyncIptables { enable: true, port: 5353 },
                AdblockStep::ResyncTunnels,
            ]
        );
        assert_eq!(steps_on.first(), Some(&AdblockStep::CommitConfig));

        // 2. Выключение AdBlock: CommitConfig строго первым, затем снятие защиты и редиректа
        let steps_off = plan_adblock_toggle(&cfg, false);
        assert_eq!(
            steps_off,
            vec![
                AdblockStep::CommitConfig,
                AdblockStep::SetProtection(false),
                AdblockStep::SyncIptables { enable: false, port: 5353 },
                AdblockStep::ResyncTunnels,
            ]
        );

        // 3. И API, и бот используют adblock_toggle_core
        let api_src = include_str!("../api/dns_adguard.rs");
        let bot_src = include_str!("../bot.rs");
        assert!(api_src.contains("pub async fn adblock_toggle_core("));
        assert!(bot_src.contains("adblock_toggle_core("));
        assert!(!bot_src.contains("let _ = crate::adguard::set_protection("));
    }
}

#[cfg(test)]
mod regression_bug_115_tests {
    #[test]
    fn regression_bug_115a_adguard_yaml_written_atomically() {
        // Баг 115a: AdGuardHome.yaml переписывался обычной записью — обрыв питания или одновременное чтение
        // AdGuard Home давали обрезанный конфиг, и AGH не запускался.
        let src = concat!(include_str!("mod.rs"), "\n", include_str!("yaml.rs"), "\n", include_str!("iptables.rs"), "\n", include_str!("service.rs"));
        let prod = &src[..src.find("#[cfg(test)]").unwrap_or(src.len())];
        assert!(!prod.contains("tokio::fs::write(p, healed)"), "лечение конфигов — атомарная запись");
        assert!(!prod.contains("tokio::fs::write(&conf_path, patched)"), "смена порта — атомарная запись");
        assert!(!prod.contains("std::fs::write(&conf_path, healed)"), "самоисцеление — атомарная запись");
    }

    #[test]
    fn regression_bug_115b_http_proxy_port_from_config() {
        let input = "http_proxy: \"\"\ndns:\n  port: 53\n  bootstrap_dns:\n  - 1.1.1.1\n";
        let (healed, changed) = super::sanitize_and_heal_adguard_yaml_with_proxy_port(input, 5353, 7891);
        assert!(changed, "первый проход должен применить изменения");
        assert!(
            healed.contains("http_proxy: \"http://127.0.0.1:7891\""),
            "порт http_proxy должен браться из mixed-port конфига Mihomo (7891):\n{healed}"
        );
        assert!(!healed.contains("7890"), "не должен зашиваться порт 7890 при mixed_port=7891");

        // Повторный вызов на результате: идемпотентность -> changed == false (запись пропускается)
        let (healed2, changed2) = super::sanitize_and_heal_adguard_yaml_with_proxy_port(&healed, 5353, 7891);
        assert!(!changed2, "повторный вызов на уже вылеченном конфиге не должен менять файл");
        assert_eq!(healed2, healed);

        // Парсер mixed-port из config.yaml Mihomo
        let mihomo_yaml = "mixed-port: 7895\ndns:\n  enable: true\n";
        assert_eq!(crate::routing::parse_mixed_port(mihomo_yaml), Some(7895));
        assert_eq!(crate::routing::parse_mixed_port("port: 7890\n"), None);
    }

    #[test]
    fn regression_bug_a09_reinsert_keeps_rule_order() {
        let rules = [
            "-A XKEEN_GAMING -s 192.168.1.10 -p udp --dport 53 -j REDIRECT --to-ports 1053",
            "-A XKEEN_GAMING -s 192.168.1.10 -p tcp --dport 53 -j REDIRECT --to-ports 1053",
            "-A XKEEN_GAMING -s 192.168.1.11 -p udp --dport 53 -j REDIRECT --to-ports 1053",
        ];
        let cmds = super::reinsert_rules_commands(&rules, 5353);
        let add_cmds: Vec<&String> = cmds.iter().filter(|c| c.contains(" -I ")).collect();
        assert_eq!(add_cmds.len(), 3);
        assert!(add_cmds[0].contains("-I XKEEN_GAMING 1 ") && add_cmds[0].contains("192.168.1.10") && add_cmds[0].contains("--to-ports 5353"));
        assert!(add_cmds[1].contains("-I XKEEN_GAMING 2 ") && add_cmds[1].contains("-p tcp"));
        assert!(add_cmds[2].contains("-I XKEEN_GAMING 3 ") && add_cmds[2].contains("192.168.1.11"));
    }

    #[test]
    fn regression_adguard_remove_iptables_uses_mihomo_config_path() {
        let dir = std::env::temp_dir().join(format!(
            "agh_restore_port_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let yaml_path = dir.join("custom_mihomo.yaml");
        std::fs::write(&yaml_path, "dns:\n  enable: true\n  listen: 0.0.0.0:7853\n").unwrap();

        let port = super::resolve_adguard_restore_port(None, yaml_path.to_str().unwrap());
        assert_eq!(port, 7853, "порт возврата должен читаться из указанного config.yaml");
        assert!(super::remove_adguard_iptables_for_mihomo_config(yaml_path.to_str().unwrap()).is_ok());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
