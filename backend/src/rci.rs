//! Клиент NDM/RCI API роутера Keenetic (порт 79 изнутри роутера).
//! Аутентификация: приоритет — токен X-Ndma-Tkn (конфиг или /opt/etc/xkeen/xkeen.json),
//! fallback — challenge-auth SHA256(challenge + md5|sha256(login:realm:password)).

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::config::AppConfig;

static AUTHED: AtomicBool = AtomicBool::new(false);

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
pub fn token_from_files(cfg: &AppConfig) -> String {
    if !cfg.rci.token.is_empty() {
        return cfg.rci.token.clone();
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(content) = std::fs::read_to_string("/opt/etc/xkeen/xkeen.json") {
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
            AUTHED.store(true, Ordering::Relaxed);
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
        AUTHED.store(true, Ordering::Relaxed);
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
        AUTHED.store(true, Ordering::Relaxed);
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
    let token = token_from_files(cfg);
    if !token.is_empty() {
        return Ok(token);
    }
    if AUTHED.load(Ordering::Relaxed) {
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
        AUTHED.store(false, Ordering::Relaxed);
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
        AUTHED.store(true, Ordering::Relaxed);
        return Ok(String::new());
    }
    Err("RCI требует авторизацию: задайте rci.token или rci.password в конфиге".into())
}

async fn rci_get(http: &reqwest::Client, cfg: &AppConfig, token: &str, path: &str) -> Result<Value, String> {
    let mut req = http
        .get(format!("{}{}", cfg.base_url(), path))
        .timeout(std::time::Duration::from_secs(4));
    if !token.is_empty() {
        req = req.header("X-Ndma-Tkn", token);
    }
    let resp = req.send().await.map_err(|e| e.to_string())?;
    let status = resp.status();
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
    let mut req = http
        .post(format!("{}{}", cfg.base_url(), path))
        .timeout(std::time::Duration::from_secs(5))
        .json(&body);
    if !token.is_empty() {
        req = req.header("X-Ndma-Tkn", token);
    }
    let resp = req.send().await.map_err(|e| e.to_string())?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        let head: String = text.chars().take(200).collect();
        return Err(format!("RCI {path}: статус {status}: {head}"));
    }
    Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
}

/// Нормализация ответа: Keenetic иногда возвращает массив с одним объектом.
fn as_object(v: Value) -> Value {
    if v.is_array() {
        v.as_array().and_then(|a| a.first().cloned()).unwrap_or(Value::Null)
    } else {
        v
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
    let runtime_hosts = match runtime {
        Some(v) => {
            if let Some(hosts) = v.get("host").and_then(|h| h.as_array()) {
                hosts.clone()
            } else if let Some(hosts) = v.as_array() {
                hosts.clone()
            } else {
                Vec::new()
            }
        }
        None => Vec::new(),
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

    let ok_final: bool;
    if policy_id.is_empty() || policy_id == "default" {
        post("/rci/ip/hotspot/host/policy".into(), json!({ "mac": mac, "no": true })).await?;
        let (_, text) = post("/rci/ip/hotspot/host".into(), json!({ "mac": mac, "access": "permit" })).await?;
        ok_final = true;
        let _ = text;
    } else if policy_id == "block" {
        let (_, text) = post("/rci/ip/hotspot/host".into(), json!({ "mac": mac, "access": "deny" })).await?;
        let _ = text;
        ok_final = true;
    } else {
        let (_, text) = post(
            "/rci/ip/hotspot/host".into(),
            json!({ "mac": mac, "policy": policy_id, "access": "permit" }),
        )
        .await?;
        let _ = text;
        ok_final = true;
    }
    let _ = ok_final;

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




