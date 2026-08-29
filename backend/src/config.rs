use serde::{Deserialize, Serialize};
use std::path::Path;

/// Конфигурация XKeen Route.
/// Хранится в /opt/etc/xkeen-route/config.json (или рядом с бинарником в dev-режиме).
/// Файл может быть частичным — недостающие поля добираются из DEFAULT (deep-merge).

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct RciConfig {
    pub host: String,
    pub port: u16,
    pub login: String,
    pub password: String,
    pub use_https: bool,
    /// RCI-токен (X-Ndma-Tkn). Если пусто — берётся из /opt/etc/xkeen/xkeen.json (xkeen.rci_token),
    /// иначе используется challenge-auth с login/password.
    pub token: String,
}

impl Default for RciConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 79,
            login: "admin".into(),
            password: String::new(),
            use_https: false,
            token: String::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct MihomoConfig {
    pub host: String,
    pub port: u16,
    pub secret: String,
    /// Путь к конфигу Mihomo на роутере (для AUTO-DEVICE маршрутизации).
    pub config_path: String,
}

impl Default for MihomoConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 9090,
            secret: String::new(),
            config_path: "/opt/etc/mihomo/config.yaml".into(),
        }
    }
}


#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct FailoverConfig {
    pub enabled: bool,
    pub ping_threshold_ms: u32,
    pub priority_server: String,
    pub auto_restore_priority: bool,
    pub interval_secs: u32,
}

impl Default for FailoverConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            ping_threshold_ms: 300,
            priority_server: String::new(),
            auto_restore_priority: true,
            interval_secs: 60,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct AppConfig {
    pub rci: RciConfig,
    pub mihomo: MihomoConfig,
    pub failover: FailoverConfig,
    pub refresh_interval_sec: u32,
    /// Игнор-лист: серверы, исключаемые из групп Fastest/Fallback (exclude-filter).
    pub ignore_servers: Vec<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            rci: RciConfig::default(),
            mihomo: MihomoConfig::default(),
            failover: FailoverConfig::default(),
            refresh_interval_sec: 10,
            ignore_servers: Vec::new(),
        }
    }
}

impl AppConfig {
    pub fn base_url(&self) -> String {
        let scheme = if self.rci.use_https { "https" } else { "http" };
        format!("{}://{}:{}", scheme, self.rci.host, self.rci.port)
    }

    pub fn mihomo_url(&self) -> String {
        format!("http://{}:{}", self.mihomo.host, self.mihomo.port)
    }
}

/// Рекурсивный merge: значения из `over` поверх `base` (объекты мержатся, остальное заменяется).
pub fn merge_value(base: &mut serde_json::Value, over: &serde_json::Value) {
    match (base, over) {
        (serde_json::Value::Object(base_map), serde_json::Value::Object(over_map)) => {
            for (k, v) in over_map {
                match base_map.get_mut(k) {
                    Some(slot) => merge_value(slot, v),
                    None => {
                        base_map.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (slot, over) => *slot = over.clone(),
    }
}

/// Загрузка конфига с deep-merge поверх дефолтов.
pub fn load(path: &Path) -> AppConfig {
    let mut base = serde_json::to_value(AppConfig::default()).unwrap_or_default();
    match std::fs::read_to_string(path) {
        Ok(content) => match serde_json::from_str::<serde_json::Value>(&content) {
            Ok(over) => merge_value(&mut base, &over),
            Err(e) => eprintln!("[WARN] {} не JSON: {} — использую дефолты", path.display(), e),
        },
        Err(_) => {}
    }
    serde_json::from_value(base).unwrap_or_else(|e| {
        eprintln!("[WARN] Ошибка конфига {}: {} — использую дефолты", path.display(), e);
        AppConfig::default()
    })
}

/// Сохранение конфига атомарно (tmp + rename).
pub async fn save(path: &Path, cfg: &AppConfig) -> Result<(), String> {
    let serialized = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    if let Some(dir) = path.parent() {
        let _ = tokio::fs::create_dir_all(dir).await;
    }
    let tmp = path.with_extension("json.tmp");
    tokio::fs::write(&tmp, serialized).await.map_err(|e| e.to_string())?;
    tokio::fs::rename(&tmp, path).await.map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deep_merge_overrides_only_given_fields() {
        let mut base = serde_json::json!({
            "failover": { "enabled": false, "ping_threshold_ms": 300 },
            "refresh_interval_sec": 10
        });
        merge_value(&mut base, &serde_json::json!({ "failover": { "enabled": true } }));
        assert_eq!(base["failover"]["enabled"], serde_json::json!(true));
        assert_eq!(base["failover"]["ping_threshold_ms"], serde_json::json!(300));
        assert_eq!(base["refresh_interval_sec"], serde_json::json!(10));
    }

    #[test]
    fn partial_config_fills_defaults() {
        let path = std::env::temp_dir().join("xr-test-config.json");
        std::fs::write(&path, r#"{"failover": {"enabled": true}}"#).unwrap();
        let cfg = load(&path);
        assert!(cfg.failover.enabled);
        assert_eq!(cfg.failover.ping_threshold_ms, 300);
        assert_eq!(cfg.rci.port, 79);
        assert_eq!(cfg.mihomo.port, 9090);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn save_and_load_roundtrip() {
        let path = std::env::temp_dir().join("xr-test-config2.json");
        let mut cfg = AppConfig::default();
        cfg.failover.ping_threshold_ms = 250;
        cfg.mihomo.secret = "s3cret".into();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(save(&path, &cfg)).unwrap();
        let loaded = load(&path);
        assert_eq!(loaded.failover.ping_threshold_ms, 250);
        assert_eq!(loaded.mihomo.secret, "s3cret");
        let _ = std::fs::remove_file(&path);
    }
}
