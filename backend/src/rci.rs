//! Клиент NDM/RCI API роутера Keenetic (порт 79 изнутри роутера).
//! Аутентификация: приоритет — токен X-Ndma-Tkn (конфиг или /opt/etc/xkeen/xkeen.json),
//! fallback — challenge-auth SHA256(challenge + md5|sha256(login:realm:password)).

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::config::AppConfig;

static AUTHED: AtomicBool = AtomicBool::new(false);
/// Файловый RCI-токен проверен и валиден (кэш, чтобы не дёргать /rci/show/version на каждый вызов).
static TOKEN_OK: AtomicBool = AtomicBool::new(false);
/// Мьютекс для синхронизации одновременных попыток авторизации к RCI
static AUTH_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone, Debug)]
pub struct Policy {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

#[derive(Clone, Debug)]
pub struct Device {
    pub mac: String,
    pub name: String,
    pub ip: String,
    pub policy: String,
    pub policy_name: String,
    pub online: bool,
    pub interface: String,
    pub is_current_device: bool,
    pub rxbytes: u64,
    pub txbytes: u64,
    pub speed_limit_kbps: u64,
    pub ipv6: Vec<String>,
}

fn md5_hex(s: &str) -> String {
    use md5::{Digest, Md5};
    let mut h = Md5::new();
    h.update(s.as_bytes());
    format!("{:x}", h.finalize())
}

fn sha256_hex(s: &str) -> String {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    hex::encode(h.finalize())
}

/// Поиск RCI-токена: config.rci.token → /opt/etc/xkeen/xkeen.json (xkeen.rci_token).
pub async fn token_from_files(cfg: &AppConfig) -> String {
    if !cfg.rci.token.is_empty() {
        return cfg.rci.token.clone();
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(content) = tokio::fs::read_to_string("/opt/etc/xkeen/xkeen.json").await {
            if let Ok(v) = serde_json::from_str::<Value>(&content) {
                if let Some(t) = v.get("xkeen").and_then(|x| x.get("rci_token")).and_then(|t| t.as_str()) {
                    return t.to_string();
                }
            }
        }
    }
    String::new()
}

/// Challenge-auth по логину/паролю (SHA256-MD5 ветка, затем SHA256-SHA256).
async fn challenge_auth(
    http: &reqwest::Client,
    base: &str,
    login: &str,
    password: &str,
) -> Result<(), String> {
    let resp = http
        .get(format!("{base}/auth"))
        .timeout(std::time::Duration::from_secs(4))
        .send()
        .await
        .map_err(|e| format!("Роутер недоступен: {e}"))?;

    let challenge = resp
        .headers()
        .get("X-NDM-Challenge")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let realm = resp
        .headers()
        .get("X-NDM-Realm")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("Keenetic")
        .to_string();

    if challenge.is_empty() {
        let check = http
            .get(format!("{base}/rci/show/version"))
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .await
            .map_err(|e| format!("Проверка сессии: {e}"))?;
        if check.status().is_success() {
            AUTHED.store(true, Ordering::Release);
            return Ok(());
        }
        return Err("Роутер не выдал challenge — задайте rci.token в конфиге".into());
    }

    // Ветка 1: SHA256(challenge + md5(login:realm:password))
    let h = sha256_hex(&format!("{challenge}{}", md5_hex(&format!("{login}:{realm}:{password}"))));
    let resp = http
        .post(format!("{base}/auth"))
        .timeout(std::time::Duration::from_secs(5))
        .json(&json!({ "login": login, "password": h }))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if resp.status().is_success() {
        AUTHED.store(true, Ordering::Release);
        return Ok(());
    }

    // Ветка 2: SHA256(challenge + sha256(login:realm:password))
    let h2 = sha256_hex(&format!("{challenge}{}", sha256_hex(&format!("{login}:{realm}:{password}"))));
    let resp = http
        .post(format!("{base}/auth"))
        .timeout(std::time::Duration::from_secs(5))
        .json(&json!({ "login": login, "password": h2 }))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if resp.status().is_success() {
        AUTHED.store(true, Ordering::Release);
        return Ok(());
    }

    Err(format!(
        "Не удалось авторизоваться в RCI (ветки MD5/SHA256 отклонены, статус {})",
        resp.status()
    ))
}

/// Гарантирует авторизацию: токен из файлов → challenge-auth → попытка без авторизации
/// (на многих прошивках RCI с localhost отвечает без auth). Возвращает токен (может быть пустым).
pub async fn ensure_auth(http: &reqwest::Client, cfg: &AppConfig) -> Result<String, String> {
    let token = token_from_files(cfg).await;
    // Быстрый путь без блокировки (Acquire ordering)
    if !token.is_empty() && TOKEN_OK.load(Ordering::Acquire) {
        return Ok(token);
    }
    if token.is_empty() && AUTHED.load(Ordering::Acquire) {
        return Ok(String::new());
    }

    // Синхронизация параллельных запросов
    let _guard = AUTH_LOCK.lock().await;

    // Повторная проверка под блокировкой (double-check)
    if !token.is_empty() {
        if TOKEN_OK.load(Ordering::Acquire) {
            return Ok(token);
        }
        // Токен из файла может устареть (RCI-сессии истекают по времени) —
        // проверяем один раз, результат кэшируется в TOKEN_OK.
        let ok = http
            .get(format!("{}/rci/show/version", cfg.base_url()))
            .timeout(std::time::Duration::from_secs(3))
            .header("X-Ndma-Tkn", &token)
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false);
        if ok {
            TOKEN_OK.store(true, Ordering::Release);
            return Ok(token);
        }
        TOKEN_OK.store(false, Ordering::Release);
        // Токен невалиден — пробуем challenge-auth ниже.
    }
    if AUTHED.load(Ordering::Acquire) {
        let check = http
            .get(format!("{}/rci/show/version", cfg.base_url()))
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .await;
        if let Ok(r) = check {
            if r.status().is_success() {
                return Ok(String::new());
            }
        }
        AUTHED.store(false, Ordering::Release);
    }
    if !cfg.rci.password.is_empty() {
        if challenge_auth(http, &cfg.base_url(), &cfg.rci.login, &cfg.rci.password)
            .await
            .is_ok()
        {
            return Ok(String::new());
        }
    }
    // Последняя попытка: RCI без авторизации (типично для localhost на KeeneticOS 4/5)
    let probe = http
        .get(format!("{}/rci/show/version", cfg.base_url()))
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await
        .map_err(|e| format!("RCI недоступен: {e}"))?;
    if probe.status().is_success() {
        AUTHED.store(true, Ordering::Release);
        return Ok(String::new());
    }
    Err("RCI требует авторизацию: задайте rci.token или rci.password в конфиге".into())
}

/// Повторная авторизация после 401/403: сброс кэша токена, challenge-auth
/// (если задан пароль) или повторный ensure_auth. Возвращает новый токен.
async fn reauth(http: &reqwest::Client, cfg: &AppConfig, _had_token: bool) -> Result<String, String> {
    let _guard = AUTH_LOCK.lock().await;
    TOKEN_OK.store(false, Ordering::Release);
    AUTHED.store(false, Ordering::Release);
    if !cfg.rci.password.is_empty()
        && challenge_auth(http, &cfg.base_url(), &cfg.rci.login, &cfg.rci.password)
            .await
            .is_ok()
    {
        return Ok(String::new());
    }
    ensure_auth(http, cfg).await
}

async fn rci_get(http: &reqwest::Client, cfg: &AppConfig, token: &str, path: &str) -> Result<Value, String> {
    match rci_get_once(http, cfg, token, path).await {
        Err(e) if e.starts_with("RCI-AUTH:") => {
            let t2 = reauth(http, cfg, !token.is_empty()).await?;
            rci_get_once(http, cfg, &t2, path).await
        }
        r => r,
    }
}

async fn rci_get_once(
    http: &reqwest::Client,
    cfg: &AppConfig,
    token: &str,
    path: &str,
) -> Result<Value, String> {
    let mut req = http
        .get(format!("{}{}", cfg.base_url(), path))
        .timeout(std::time::Duration::from_secs(4));
    if !token.is_empty() {
        req = req.header("X-Ndma-Tkn", token);
    }
    let resp = req.send().await.map_err(|e| e.to_string())?;
    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(format!("RCI-AUTH: {path}: статус {status}"));
    }
    let body: Value = resp.json().await.map_err(|e| format!("RCI {path}: {e}"))?;
    if !status.is_success() {
        return Err(format!("RCI {path}: статус {status}"));
    }
    Ok(body)
}

async fn rci_post(
    http: &reqwest::Client,
    cfg: &AppConfig,
    token: &str,
    path: &str,
    body: Value,
) -> Result<Value, String> {
    match rci_post_once(http, cfg, token, path, body.clone()).await {
        Err(e) if e.starts_with("RCI-AUTH:") => {
            let t2 = reauth(http, cfg, !token.is_empty()).await?;
            rci_post_once(http, cfg, &t2, path, body).await
        }
        r => r,
    }
}

async fn rci_post_once(
    http: &reqwest::Client,
    cfg: &AppConfig,
    token: &str,
    path: &str,
    body: Value,
) -> Result<Value, String> {
    let mut req = http
        .post(format!("{}{}", cfg.base_url(), path))
        .timeout(std::time::Duration::from_secs(5))
        .json(&body);
    if !token.is_empty() {
        req = req.header("X-Ndma-Tkn", token);
    }
    let resp = req.send().await.map_err(|e| e.to_string())?;
    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(format!("RCI-AUTH: {path}: статус {status}"));
    }
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        let head: String = text.chars().take(200).collect();
        return Err(format!("RCI {path}: статус {status}: {head}"));
    }
    serde_json::from_str(&text).map_err(|e| {
        let head: String = text.chars().take(200).collect();
        crate::log_w!("Ошибка парсинга JSON RCI ответа {path}: {e} (ответ: {head})");
        format!("RCI {path}: ошибка парсинга JSON: {e}")
    })
}

/// Нормализация ответа: Keenetic иногда возвращает массив с одним объектом.
fn as_object(v: Value) -> Value {
    match v {
        Value::Array(mut arr) if !arr.is_empty() => arr.swap_remove(0),
        Value::Array(_) => Value::Object(Default::default()),
        other => other,
    }
}

/// Модель/версия KeeneticOS из /rci/show/version.
pub async fn get_version(http: &reqwest::Client, cfg: &AppConfig) -> Result<BTreeMap<String, String>, String> {
    let token = ensure_auth(http, cfg).await?;
    let v = as_object(rci_get(http, cfg, &token, "/rci/show/version").await?);
    let mut out = BTreeMap::new();
    if let Some(o) = v.as_object() {
        for key in ["model", "version", "hostname", "serial"] {
            if let Some(s) = o.get(key).and_then(|x| x.as_str()) {
                out.insert(key.to_string(), s.to_string());
            }
        }
        // Keenetic не отдаёт поле "version" — берём человекочитаемый title или release
        if !out.contains_key("version") {
            if let Some(s) = o.get("title").and_then(|x| x.as_str()) {
                out.insert("version".into(), format!("KeeneticOS {s}"));
            } else if let Some(s) = o.get("release").and_then(|x| x.as_str()) {
                out.insert("version".into(), format!("KeeneticOS {s}"));
            }
        }
        if let Some(u) = o.get("uptime").and_then(|x| x.as_u64()) {
            out.insert("uptime".into(), u.to_string());
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct SystemStats {
    pub cpu_percent: u32,
    pub memory_used_mb: u32,
    pub memory_total_mb: u32,
    #[serde(default)]
    pub app_memory_mb: f64,
    #[serde(default)]
    pub app_cpu_percent: f64,
    #[serde(default)]
    pub core_memory_mb: f64,
    #[serde(default)]
    pub total_xkeen_memory_mb: f64,
}

/// Получение показателей потребления физической памяти (RSS в МБ) процесса по его имени.
pub fn get_process_rss(proc_name: &str) -> f64 {
    #[cfg(target_os = "linux")]
    {
        if let Ok(entries) = std::fs::read_dir("/proc") {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.chars().all(|c| c.is_ascii_digit()) {
                        let comm_path = path.join("comm");
                        if let Ok(comm) = std::fs::read_to_string(&comm_path) {
                            if comm.trim() == proc_name {
                                let status_path = path.join("status");
                                if let Ok(s) = std::fs::read_to_string(&status_path) {
                                    for line in s.lines() {
                                        if line.starts_with("VmRSS:") {
                                            let parts: Vec<&str> = line.split_whitespace().collect();
                                            if parts.len() >= 2 {
                                                if let Ok(kb) = parts[1].parse::<f64>() {
                                                    return (kb / 1024.0 * 10.0).round() / 10.0;
                                                }
                                            }
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        0.0
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = proc_name;
        0.0
    }
}

/// Получение показателей потребления собственного процесса (память RSS в МБ и CPU%).
pub fn get_proc_stats() -> (f64, f64) {
    #[cfg(target_os = "linux")]
    {
        let mut rss_mb = 0.0;
        if let Ok(s) = std::fs::read_to_string("/proc/self/status") {
            for line in s.lines() {
                if line.starts_with("VmRSS:") {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 2 {
                        if let Ok(kb) = parts[1].parse::<f64>() {
                            rss_mb = (kb / 1024.0 * 10.0).round() / 10.0;
                        }
                    }
                    break;
                }
            }
        }

        static LAST_CPU: std::sync::Mutex<Option<(std::time::Instant, u64)>> = std::sync::Mutex::new(None);
        let mut cpu_pct = 0.0;

        if let Ok(stat_line) = std::fs::read_to_string("/proc/self/stat") {
            let tokens: Vec<&str> = stat_line.split_whitespace().collect();
            if tokens.len() >= 15 {
                let utime: u64 = tokens[13].parse().unwrap_or(0);
                let stime: u64 = tokens[14].parse().unwrap_or(0);
                let current_ticks = utime + stime;
                let now = std::time::Instant::now();

                if let Ok(mut guard) = LAST_CPU.lock() {
                    if let Some((prev_time, prev_ticks)) = *guard {
                        let elapsed = now.duration_since(prev_time).as_secs_f64();
                        if elapsed >= 0.2 && current_ticks >= prev_ticks {
                            let delta_ticks = (current_ticks - prev_ticks) as f64;
                            let p = (delta_ticks / (100.0 * elapsed)) * 100.0;
                            cpu_pct = (p * 10.0).round() / 10.0;
                        }
                    }
                    *guard = Some((now, current_ticks));
                }
            }
        }

        (rss_mb, cpu_pct)
    }
    #[cfg(not(target_os = "linux"))]
    {
        (0.0, 0.0)
    }
}

/// Статистика нагрузки CPU и RAM из /rci/show/system + процесс xkeen-route + ядро mihomo.
pub async fn get_system(http: &reqwest::Client, cfg: &AppConfig) -> Result<SystemStats, String> {
    let token = ensure_auth(http, cfg).await?;
    let v = as_object(rci_get(http, cfg, &token, "/rci/show/system").await?);
    if let Some(o) = v.as_object() {
        let cpu_percent = o.get("cpuload").and_then(|c| c.as_u64()).unwrap_or(0) as u32;
        let mut memory_used_mb = 0;
        let mut memory_total_mb = 0;
        if let Some(mem_str) = o.get("memory").and_then(|m| m.as_str()) {
            let parts: Vec<&str> = mem_str.split('/').collect();
            if parts.len() == 2 {
                let used_kb: u64 = parts[0].parse().unwrap_or(0);
                let total_kb: u64 = parts[1].parse().unwrap_or(0);
                memory_used_mb = (used_kb / 1024) as u32;
                memory_total_mb = (total_kb / 1024) as u32;
            }
        }
        if memory_total_mb == 0 {
            if let Some(tot_kb) = o.get("memtotal").and_then(|m| m.as_u64()) {
                memory_total_mb = (tot_kb / 1024) as u32;
            }
        }
        let (app_memory_mb, app_cpu_percent) = get_proc_stats();
        let core_memory_mb = get_process_rss(&cfg.mihomo.process_name);
        let total_xkeen_memory_mb = ((app_memory_mb + core_memory_mb) * 10.0).round() / 10.0;
        return Ok(SystemStats {
            cpu_percent,
            memory_used_mb,
            memory_total_mb,
            app_memory_mb,
            app_cpu_percent,
            core_memory_mb,
            total_xkeen_memory_mb,
        });
    }
    Err("Invalid system response".into())
}

/// Список политик доступа (+default, +block) с иконками — как в десктопе.
pub async fn get_policies(http: &reqwest::Client, cfg: &AppConfig) -> Result<Vec<Policy>, String> {
    let token = ensure_auth(http, cfg).await?;
    let mut map: BTreeMap<String, Policy> = BTreeMap::new();
    map.insert(
        "default".into(),
        Policy { id: "default".into(), name: "🌐 Политика по умолчанию".into(), is_default: true },
    );

    for path in ["/rci/show/ip/policy", "/rci/ip/policy"] {
        if let Ok(v) = rci_get(http, cfg, &token, path).await {
            let obj = as_object(v);
            if let Some(o) = obj.as_object() {
                for (pol_id, pol_data) in o {
                    if map.contains_key(pol_id) {
                        continue;
                    }
                    let desc = pol_data
                        .get("description")
                        .and_then(|d| d.as_str())
                        .unwrap_or(pol_id)
                        .to_string();
                    let lower = desc.to_lowercase();
                    let icon = if lower.contains("xkeen") {
                        "🛡️"
                    } else if lower.contains("multipath") {
                        "⚡"
                    } else {
                        "🔹"
                    };
                    map.insert(
                        pol_id.clone(),
                        Policy { id: pol_id.clone(), name: format!("{icon} {desc}"), is_default: false },
                    );
                }
            }
        }
    }

    map.insert(
        "block".into(),
        Policy { id: "block".into(), name: "🚫 Без доступа в интернет".into(), is_default: false },
    );
    Ok(map.into_values().collect())
}

/// Устройства: 3 параллельных RCI-запроса + склейка по MAC (порт get_devices).
pub async fn get_devices(
    http: &reqwest::Client,
    cfg: &AppConfig,
    policies: &[Policy],
    current_client_ip: &str,
) -> Result<Vec<Device>, String> {
    let token = ensure_auth(http, cfg).await?;
    let base = cfg.base_url();
    let tk = token.clone();

    let mk_get = |path: &'static str| {
        let http = http.clone();
        let base = base.clone();
        let tk = tk.clone();
        async move {
            let mut req = http.get(format!("{base}{path}")).timeout(std::time::Duration::from_secs(4));
            if !tk.is_empty() {
                req = req.header("X-Ndma-Tkn", &tk);
            }
            let resp = req.send().await.ok()?;
            resp.json::<Value>().await.ok()
        }
    };

    let (hotspot_cfg, shape, runtime) = tokio::join!(
        mk_get("/rci/ip/hotspot"),
        mk_get("/rci/ip/traffic-shape"),
        mk_get("/rci/show/ip/hotspot")
    );

    // MAC → {policy, access}
    let mut mac_policy: BTreeMap<String, (String, String)> = BTreeMap::new();
    if let Some(hosts) = hotspot_cfg.as_ref().and_then(|v| v.get("host")).and_then(|h| h.as_array()) {
        for item in hosts {
            if let Some(mac) = item.get("mac").and_then(|m| m.as_str()) {
                mac_policy.insert(
                    mac.to_lowercase(),
                    (
                        item.get("policy").and_then(|p| p.as_str()).unwrap_or("").to_string(),
                        item.get("access").and_then(|a| a.as_str()).unwrap_or("permit").to_string(),
                    ),
                );
            }
        }
    }

    // MAC → rate
    let mut mac_shape: BTreeMap<String, u64> = BTreeMap::new();
    if let Some(hosts) = shape.as_ref().and_then(|v| v.get("host")).and_then(|h| h.as_array()) {
        for item in hosts {
            if let (Some(mac), Some(rate)) = (
                item.get("mac").and_then(|m| m.as_str()),
                item.get("rate").and_then(|r| r.as_u64()),
            ) {
                if rate > 0 {
                    mac_shape.insert(mac.to_lowercase(), rate);
                }
            }
        }
    }

    // Runtime-хосты
    let runtime_val = runtime.ok_or_else(|| {
        "Таймаут или ошибка получения списка устройств через RCI (/rci/show/ip/hotspot)".to_string()
    })?;
    let runtime_hosts = if let Some(hosts) = runtime_val.get("host").and_then(|h| h.as_array()) {
        hosts.clone()
    } else if let Some(hosts) = runtime_val.as_array() {
        hosts.clone()
    } else {
        Vec::new()
    };

    let mut devices: Vec<Device> = Vec::new();
    for h in &runtime_hosts {
        let mac = h.get("mac").and_then(|m| m.as_str()).unwrap_or("").to_lowercase();
        if mac.is_empty() {
            continue;
        }
        let name = ["name", "hostname", "ip"]
            .iter()
            .find_map(|k| h.get(*k).and_then(|v| v.as_str()))
            .unwrap_or("Устройство")
            .to_string();
        let ip = h.get("ip").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let online = h
            .get("active")
            .and_then(|a| a.as_bool())
            .unwrap_or_else(|| h.get("link").and_then(|l| l.as_str()).map(|s| !s.is_empty()).unwrap_or(false));

        let (cfg_policy, cfg_access) = mac_policy
            .get(&mac)
            .cloned()
            .unwrap_or_else(|| (String::new(), "permit".into()));
        let raw_policy = if !cfg_policy.is_empty() {
            cfg_policy
        } else {
            h.get("policy").and_then(|p| p.as_str()).unwrap_or("").to_string()
        };
        let access = if cfg_access != "permit" { cfg_access } else { "permit".into() };

        let (pol_id, pol_name) = if access == "deny" {
            ("block".to_string(), "🚫 Без доступа в интернет".to_string())
        } else if let Some(p) = policies.iter().find(|p| p.id == raw_policy) {
            (raw_policy.clone(), p.name.clone())
        } else if !raw_policy.is_empty() {
            (raw_policy.clone(), format!("🔹 {raw_policy}"))
        } else {
            ("default".to_string(), "🌐 Политика по умолчанию".to_string())
        };

        let mut speed = mac_shape.get(&mac).copied().unwrap_or(0);
        if speed == 0 {
            speed = h
                .get("traffic-shape")
                .and_then(|t| t.get("rx"))
                .and_then(|r| r.as_u64())
                .unwrap_or(0);
        }

        let mut ipv6_addrs = Vec::new();
        if let Some(v6_val) = h.get("ipv6") {
            if let Some(arr) = v6_val.as_array() {
                for item in arr {
                    if let Some(s) = item.as_str() {
                        ipv6_addrs.push(s.to_string());
                    } else if let Some(s) = item.get("address").and_then(|a| a.as_str()) {
                        ipv6_addrs.push(s.to_string());
                    } else if let Some(s) = item.get("ip").and_then(|a| a.as_str()) {
                        ipv6_addrs.push(s.to_string());
                    }
                }
            } else if let Some(s) = v6_val.as_str() {
                ipv6_addrs.push(s.to_string());
            }
        }

        devices.push(Device {
            is_current_device: !ip.is_empty() && ip == current_client_ip,
            interface: h
                .get("interface")
                .and_then(|i| i.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .to_string(),
            mac,
            name,
            ip,
            policy: pol_id,
            policy_name: pol_name,
            online,
            rxbytes: h.get("rxbytes").and_then(|v| v.as_u64()).unwrap_or(0),
            txbytes: h.get("txbytes").and_then(|v| v.as_u64()).unwrap_or(0),
            speed_limit_kbps: speed,
            ipv6: ipv6_addrs,
        });
    }

    devices.sort_by(|a, b| {
        (!a.is_current_device)
            .cmp(&(!b.is_current_device))
            .then((!a.online).cmp(&(!b.online)))
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(devices)
}

/// Назначение политики устройству (default / block / custom) — порт set_device_policy.
pub async fn set_device_policy(
    http: &reqwest::Client,
    cfg: &AppConfig,
    mac: &str,
    policy_id: &str,
    save: bool,
) -> Result<String, String> {
    let token = ensure_auth(http, cfg).await?;
    let mac = mac.to_lowercase();
    let base = cfg.base_url();

    let post = |path: String, body: Value| {
        let http = http.clone();
        let base = base.clone();
        let token = token.clone();
        async move {
            let mut req = http.post(format!("{base}{path}")).timeout(std::time::Duration::from_secs(5)).json(&body);
            if !token.is_empty() {
                req = req.header("X-Ndma-Tkn", &token);
            }
            let resp = req.send().await.map_err(|e| e.to_string())?;
            let ok = resp.status().is_success();
            let text = resp.text().await.unwrap_or_default();
            Ok::<_, String>((ok, text))
        }
    };

    if policy_id.is_empty() || policy_id == "default" {
        post("/rci/ip/hotspot/host/policy".into(), json!({ "mac": mac, "no": true })).await?;
        let _ = post("/rci/ip/hotspot/host".into(), json!({ "mac": mac, "access": "permit" })).await?;
    } else if policy_id == "block" {
        let _ = post("/rci/ip/hotspot/host".into(), json!({ "mac": mac, "access": "deny" })).await?;
    } else {
        let _ = post(
            "/rci/ip/hotspot/host".into(),
            json!({ "mac": mac, "policy": policy_id, "access": "permit" }),
        )
        .await?;
    }

    if save {
        save_config(http, cfg).await?;
    }
    Ok(format!("Политика изменена на '{policy_id}'"))
}

/// Лимит скорости (кбит/с, 0 = снять) — порт set_device_speed_limit.
pub async fn set_device_speed(
    http: &reqwest::Client,
    cfg: &AppConfig,
    mac: &str,
    kbps: u64,
    save: bool,
) -> Result<String, String> {
    let token = ensure_auth(http, cfg).await?;
    let mac = mac.to_lowercase();
    let body = if kbps > 0 {
        json!({ "mac": mac, "rate": kbps })
    } else {
        json!({ "mac": mac, "no": true })
    };
    rci_post(http, cfg, &token, "/rci/ip/traffic-shape/host", body).await?;
    if save {
        save_config(http, cfg).await?;
    }
    Ok("Ограничение скорости применено".into())
}

/// Сохранение running-config в startup-config.
pub async fn save_config(http: &reqwest::Client, cfg: &AppConfig) -> Result<(), String> {
    let token = ensure_auth(http, cfg).await?;
    rci_post(http, cfg, &token, "/rci/system/configuration/save", json!({})).await?;
    Ok(())
}

/// Настройка чистых DNS-серверов в KeeneticOS (Яндекс DNS 77.88.8.8, 77.88.8.1, Cloudflare 1.1.1.1)
/// для устранения отравления DNS провайдером (NXDOMAIN для YouTube и сайтов).
pub async fn set_clean_dns_servers(http: &reqwest::Client, cfg: &AppConfig) -> Result<(), String> {
    let token = ensure_auth(http, cfg).await?;
    let _ = rci_post(http, cfg, &token, "/rci/ip/name-server", json!({ "address": "77.88.8.8" })).await;
    let _ = rci_post(http, cfg, &token, "/rci/ip/name-server", json!({ "address": "77.88.8.1" })).await;
    let _ = rci_post(http, cfg, &token, "/rci/ip/name-server", json!({ "address": "1.1.1.1" })).await;
    let _ = save_config(http, cfg).await;
    Ok(())
}




