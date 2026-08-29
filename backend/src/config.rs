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
    /// Провайдеры, подключаемые к группам устройств (use:). Пусто = взять все
    /// proxy-providers из config.yaml автоматически.
    pub device_providers: Vec<String>,
}

impl Default for MihomoConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 9090,
            secret: String::new(),
            config_path: "/opt/etc/mihomo/config.yaml".into(),
            device_providers: Vec::new(),
        }
    }
}

/// Сервис и бэкапы (универсальные пути; на Entware — стандартные).
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct SystemConfig {
    /// Init-скрипт сервиса XKeen (start/stop/restart возвращают исходный конфиг).
    pub xkeen_init: String,
    /// Каталог бэкапов (как в XKeen-UI: /opt/backups).
    pub backup_dir: String,
}

impl Default for SystemConfig {
    fn default() -> Self {
        Self {
            xkeen_init: "/opt/etc/init.d/S05xkeen".into(),
            backup_dir: if cfg!(target_os = "linux") {
                "/opt/backups".into()
            } else {
                "backups".into()
            },
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
    /// Per-device failover: мониторинг цепочек server+резервы (device_routing).
    pub device_failover_enabled: bool,
}

impl Default for FailoverConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            ping_threshold_ms: 300,
            priority_server: String::new(),
            auto_restore_priority: true,
            interval_secs: 60,
            device_failover_enabled: false,
        }
    }
}

/// Цепочка серверов для устройства: [основной, резерв1, резерв2, ...].
/// Ключ — IP устройства. Порог 0 = наследовать глобальный failover.ping_threshold_ms.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct DeviceRouting {
    pub servers: Vec<String>,
    pub ping_threshold_ms: u32,
    pub auto_restore: bool,
}

impl Default for DeviceRouting {
    fn default() -> Self {
        Self {
            servers: Vec::new(),
            ping_threshold_ms: 300,
            auto_restore: true,
        }
    }
}

/// Настройки логирования.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct LogsConfig {
    /// Минимальный уровень: "info" | "warn" | "error".
    pub level: String,
    /// Удалённый syslog "host:port" (UDP, RFC 3164). Пусто = не отправлять.
    pub remote_syslog: String,
    /// Логировать HTTP-запросы к панели.
    pub log_requests: bool,
}

impl Default for LogsConfig {
    fn default() -> Self {
        Self {
            level: "info".into(),
            remote_syslog: String::new(),
            log_requests: true,
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
    /// Original provider exclude-filters (restored when ignore list is cleared).
    pub provider_filters: std::collections::BTreeMap<String, String>,
    /// Per-device цепочки серверов (основной + резервы). Ключ — IP устройства.
    pub device_routing: std::collections::BTreeMap<String, DeviceRouting>,
    /// Домены, которые всегда идут напрямую (мимо прокси).
    pub direct_domains: Vec<String>,
    /// Домены, которые всегда принудительно через прокси.
    pub force_domains: Vec<String>,
    /// Сервис XKeen и бэкапы.
    pub system: SystemConfig,
    /// Логирование.
    pub logs: LogsConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            rci: RciConfig::default(),
            mihomo: MihomoConfig::default(),
            failover: FailoverConfig::default(),
            refresh_interval_sec: 10,
            ignore_servers: Vec::new(),
            provider_filters: std::collections::BTreeMap::new(),
            device_routing: std::collections::BTreeMap::new(),
            direct_domains: Vec::new(),
            force_domains: Vec::new(),
            system: SystemConfig::default(),
            logs: LogsConfig::default(),
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
