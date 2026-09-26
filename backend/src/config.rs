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
    pub mixed_port: u16,
    pub secret: String,
    /// Путь к конфигу Mihomo на роутере (для AUTO-DEVICE маршрутизации).
    pub config_path: String,
    /// Путь к файлу логов Mihomo.
    pub log_path: String,
    /// Имя процесса Mihomo в системе (mihomo / clash / clash-meta).
    pub process_name: String,
    /// Провайдеры, подключаемые к группам устройств (use:). Пусто = взять все
    /// proxy-providers из config.yaml автоматически.
    pub device_providers: Vec<String>,
    /// URL проверки доступности (health-check) для прокси и подписок.
    pub health_check_url: String,
    /// Интервал health-check для подписок (сек).
    pub health_check_interval: u32,
}

impl Default for MihomoConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 9090,
            mixed_port: 7890,
            secret: String::new(),
            config_path: "/opt/etc/mihomo/config.yaml".into(),
            log_path: "/opt/var/log/mihomo.log".into(),
            process_name: "mihomo".into(),
            device_providers: Vec::new(),
            health_check_url: "http://www.gstatic.com/generate_204".into(),
            health_check_interval: 300,
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


#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct FailoverPresetSlot {
    pub id: String,
    pub name: String,
    pub chain: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct FailoverConfig {
    pub enabled: bool,
    pub ping_threshold_ms: u32,
    pub priority_server: String,
    /// Глобальная цепочка приоритетов: [основной, резерв1, ...].
    /// Миграция: если пуста, но задан priority_server — инициализируется из него.
    pub priority_chain: Vec<String>,
    pub auto_restore_priority: bool,
    pub interval_secs: u32,
    /// Per-device failover: мониторинг цепочек server+резервы (device_routing).
    pub device_failover_enabled: bool,
    /// Именованные слоты настроенных пресетов для мгновенного переключения
    pub presets: Vec<FailoverPresetSlot>,
}

impl Default for FailoverConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            ping_threshold_ms: 300,
            priority_server: String::new(),
            priority_chain: Vec::new(),
            auto_restore_priority: true,
            interval_secs: 60,
            device_failover_enabled: false,
            presets: Vec::new(),
        }
    }
}

impl FailoverConfig {
    /// Обратная совместимость: пустая цепочка + заданный одиночный приоритет → цепочка из него.
    /// Также синхронизирует priority_server = первый элемент цепочки.
    pub fn migrate_priority(&mut self) {
        if self.priority_chain.is_empty() && !self.priority_server.is_empty() {
            self.priority_chain = vec![self.priority_server.clone()];
        }
        if !self.priority_chain.is_empty() {
            self.priority_server = self.priority_chain[0].clone();
        } else {
            self.priority_server.clear();
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
pub struct AuthConfig {
    pub enabled: bool,
    pub password_hash: String,
    pub salt: String,
    pub session_secret: String,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            password_hash: String::new(),
            salt: String::new(),
            session_secret: String::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct NotificationsConfig {
    pub telegram_enabled: bool,
    pub telegram_bot_token: String,
    pub telegram_chat_id: String,
    pub webhook_url: String,
}

impl Default for NotificationsConfig {
    fn default() -> Self {
        Self {
            telegram_enabled: false,
            telegram_bot_token: String::new(),
            telegram_chat_id: String::new(),
            webhook_url: String::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct DeviceDomainRule {
    pub domain: String,
    pub target: String,
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
    /// Кастомные привязки доменов к устройствам: IP -> [ { domain, target } ]
    pub device_domain_rules: std::collections::BTreeMap<String, Vec<DeviceDomainRule>>,
    /// Домены, которые всегда идут напрямую (мимо прокси).
    pub direct_domains: Vec<String>,
    /// Домены, которые всегда принудительно через прокси.
    pub force_domains: Vec<String>,
    /// Сервис XKeen и бэкапы.
    pub system: SystemConfig,
    /// Логирование.
    pub logs: LogsConfig,
    /// Авторизация и безопасность.
    pub auth: AuthConfig,
    /// Уведомления (Telegram / Webhook).
    pub notifications: NotificationsConfig,
    /// Пользовательские названия подписок (провайдеров): provider_id -> alias.
    pub provider_aliases: std::collections::BTreeMap<String, String>,
    /// Настройки обхода блокировки Google Antigravity (Cloud Code API).
    pub antigravity: AntigravityConfig,
    /// Выделенный сервер для Google Flow & AI сервисов (не затрагивает основной PROXY / Failover).
    pub flow_server: Option<String>,
    /// Блокировка рекламы на роутере через Mihomo (GEOSITE,category-ads-all,REJECT).
    pub adblock_enabled: bool,
    /// Расписания работы устройств (блокировка/прокси/direct по часам).
    pub schedules: Vec<DeviceSchedule>,
    /// Умные режимы и гибридная маршрутизация Zapret DPI.
    pub zapret: ZapretConfig,
    /// Отдельный модуль игрового режима (обход игровых блокировок).
    pub gaming: GamingConfig,
}

/// Платформы и сервисы для игрового режима.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct GamingPlatforms {
    pub discord: bool,
    pub steam: bool,
    pub playstation: bool,
    pub xbox: bool,
    pub battlenet: bool,
    pub epicgames: bool,
    pub ea: bool,
    pub riot: bool,
    pub supercell: bool,
    pub nintendo: bool,
    pub roblox: bool,
    pub category_games: bool,
}

impl Default for GamingPlatforms {
    fn default() -> Self {
        Self {
            discord: true,
            steam: true,
            playstation: true,
            xbox: true,
            battlenet: true,
            epicgames: true,
            ea: true,
            riot: true,
            supercell: true,
            nintendo: true,
            roblox: true,
            category_games: false,
        }
    }
}

/// Режим работы игрового маршрута. Compatibility направляет все новые
/// соединения выбранного устройства через отдельную группу Mihomo.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GamingMode {
    Compatibility,
    SmartSplit,
    KnownServices,
}

impl Default for GamingMode {
    fn default() -> Self {
        Self::Compatibility
    }
}

/// Игровое LAN-устройство. MAC — постоянный идентификатор, IP/IPv6 — последний
/// известный адрес; перед активацией они обновляются по данным Keenetic.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(default)]
pub struct GamingDevice {
    pub mac: String,
    pub ip: String,
    pub ipv6: Vec<String>,
    pub name: String,
    pub enabled: bool,
}

/// Связь игрового режима с Zapret. Предыдущее состояние хранится только для
/// управляемого запуска, чтобы выключение игрового режима не меняло вручную
/// настроенный Zapret пользователя.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct GamingZapretPolicy {
    pub managed: bool,
    pub enabled: bool,
    pub preset: String,
    pub restore_previous_state: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_config: Option<ZapretConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_running: Option<bool>,
}

impl Default for GamingZapretPolicy {
    fn default() -> Self {
        Self {
            managed: true,
            enabled: true,
            preset: "gaming_compat".into(),
            restore_previous_state: true,
            previous_config: None,
            previous_running: None,
        }
    }
}

/// Настройки игрового режима (обход блокировок игр через VPS/Mihomo).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct GamingConfig {
    pub enabled: bool,
    pub mode: GamingMode,
    /// Выходной сервер: "Fastest", "PROXY", или имя конкретной ноды.
    pub target_server: String,
    /// Устройства, для которых действует игровой режим.
    pub devices: Vec<GamingDevice>,
    /// Флаги включения игровых платформ.
    pub platforms: GamingPlatforms,
    /// Дополнительные пользовательские домены.
    pub custom_domains: Vec<String>,
    /// Применять совместимый DNS redir-host, если конфигурация DNS поддерживает его.
    pub force_redir_host_for_devices: bool,
    /// Не использовать Fake-IP для известных игровых доменов.
    pub exclude_fake_ip_for_game_domains: bool,
    /// Автоматическое управление Zapret из Gaming Mode.
    pub zapret: GamingZapretPolicy,
    /// Устаревший флаг, сохраняется для чтения старых конфигов. Логика задаётся mode.
    pub smart_split: bool,
    /// Устаревший флаг, сохраняется для совместимости со старым config.json.
    pub fix_nat_fake_ip: bool,
}

impl Default for GamingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: GamingMode::Compatibility,
            target_server: "Fastest".into(),
            devices: Vec::new(),
            platforms: GamingPlatforms::default(),
            custom_domains: Vec::new(),
            force_redir_host_for_devices: false,
            exclude_fake_ip_for_game_domains: true,
            zapret: GamingZapretPolicy::default(),
            smart_split: true,
            fix_nat_fake_ip: true,
        }
    }
}

/// Пользовательская запись сайта для обхода DPI со связанными CDN серверами (/boost).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct ZapretCustomEntry {
    pub domain: String,
    pub enabled: bool,
    pub cdns: Vec<String>,
}

impl Default for ZapretCustomEntry {
    fn default() -> Self {
        Self {
            domain: String::new(),
            enabled: true,
            cdns: Vec::new(),
        }
    }
}

/// Нормализует введенный адрес сайта в чистый FQDN (домен без протокола, порта, пути, query, www и концевых точек).
pub fn normalize_domain(input: &str) -> String {
    let lower = input.trim().to_lowercase();
    let no_proto = if let Some(idx) = lower.find("://") {
        &lower[idx + 3..]
    } else {
        &lower
    };
    let host_part = no_proto
        .split('/')
        .next()
        .unwrap_or("")
        .split('?')
        .next()
        .unwrap_or("")
        .split('#')
        .next()
        .unwrap_or("");
    let no_user = host_part.split('@').last().unwrap_or(host_part);
    let no_port = no_user.split(':').next().unwrap_or(no_user);
    let clean = no_port
        .trim_start_matches("www.")
        .trim_matches(|c: char| c == '.' || c.is_whitespace());
    clean.to_string()
}

/// Проверяет корректность публичного домена для обхода цензуры (исключает локальные сети, IP-адреса и спецсимволы).
pub fn is_valid_domain(domain: &str) -> bool {
    if domain.is_empty() || domain.len() > 253 {
        return false;
    }
    if domain == "localhost"
        || domain.ends_with(".local")
        || domain.ends_with(".lan")
        || domain.ends_with(".internal")
        || domain.ends_with(".onion")
    {
        return false;
    }
    // Отклоняем IP-адреса (IPv4 / IPv6) — они не должны попадать в SNI hostlist
    if domain.parse::<std::net::IpAddr>().is_ok() {
        return false;
    }
    if !domain.contains('.') {
        return false;
    }
    for label in domain.split('.') {
        if label.is_empty() || label.len() > 63 {
            return false;
        }
        if label.starts_with('-') || label.ends_with('-') {
            return false;
        }
        if !label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return false;
        }
    }
    true
}

/// Умные режимы и гибридная маршрутизация Zapret DPI.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct ZapretConfig {
    pub enabled: bool,
    /// Гибридный режим: YouTube -> DIRECT (максимальная скорость с локальных кэшей GGC без расхода VPS)
    pub hybrid_youtube: bool,
    /// Гибридный режим: Discord -> DIRECT (минимальный пинг, прямые шлюзы)
    pub hybrid_discord: bool,
    /// Перехват голосовых каналов Discord (UDP 50000:65535) в iptables
    pub discord_voice_udp: bool,
    /// Турбо-десинхронизация YouTube (disorder2 вместо split2)
    pub youtube_turbo: bool,
    /// Универсальный обход DPI (fake,split2 на портах 80,443)
    pub general_bypass: bool,
    /// Агрессивный режим DPI для жестких ТСПУ (seqovl=1, midsld, badseq)
    pub aggressive_dpi: bool,
    /// Принудительная изоляция заблокированных ресурсов (ChatGPT, Claude, X/Twitter, Instagram) через PROXY
    pub isolated_proxy: bool,
    /// Прямой обход GitHub (github.com, raw, assets, codeload, api)
    pub bypass_github: bool,
    /// Прямой обход популярных трекеров (RuTracker, Kinozal, Rutor, Flibusta, NNMClub)
    pub bypass_torrents: bool,
    /// Прямой обход 18+ контента (Pornhub, Xvideos, XHamster, XNXX, RedTube, YouPorn)
    pub bypass_adult: bool,
    /// Пользовательские сайты с автоматическим обнаружением CDN (/boost)
    pub custom_entries: Vec<ZapretCustomEntry>,
    /// Кастомные аргументы nfqws (сохраняются при установке пресетов)
    pub custom_args: Option<String>,
}

impl Default for ZapretConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            hybrid_youtube: true,
            hybrid_discord: true,
            discord_voice_udp: true,
            youtube_turbo: true,
            general_bypass: true,
            aggressive_dpi: false,
            isolated_proxy: true,
            bypass_github: true,
            bypass_torrents: true,
            bypass_adult: true,
            custom_entries: Vec::new(),
            custom_args: None,
        }
    }
}

/// Расписание работы устройства (блокировка/прокси/direct по времени).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct DeviceSchedule {
    pub id: String,
    pub ip: String,
    pub enabled: bool,
    pub time_start: String, // "23:00"
    pub time_end: String,   // "07:00"
    pub days: Vec<u8>,      // 1..=7 (1=Пн, 7=Вс)
    pub action: String,     // "block" | "direct" | "proxy"
    pub target_server: Option<String>,
}

impl Default for DeviceSchedule {
    fn default() -> Self {
        Self {
            id: String::new(),
            ip: String::new(),
            enabled: true,
            time_start: "23:00".into(),
            time_end: "07:00".into(),
            days: vec![1, 2, 3, 4, 5, 6, 7],
            action: "block".into(),
            target_server: None,
        }
    }
}

/// Настройки обхода блокировки Google Antigravity / Cloud Code API.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct AntigravityConfig {
    pub enabled: bool,
    pub mode: String, // "auto" | "direct" | "vpn" | "proxy"
    pub proxy_port: u16,
    pub proxy_enabled: bool,
    pub health_check_interval: u64,
    pub wan_interface: String,
    pub vpn_interface: String,
    pub own_proxy: String,
    pub targets: Vec<String>,
}

impl Default for AntigravityConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: "auto".into(),
            proxy_port: 53129,
            proxy_enabled: true,
            health_check_interval: 120,
            wan_interface: "apcli1".into(),
            vpn_interface: "nwg0".into(),
            own_proxy: String::new(),
            targets: vec![
                "cloudcode-pa.googleapis.com".into(),
            ],
        }
    }
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
            device_domain_rules: std::collections::BTreeMap::new(),
            direct_domains: Vec::new(),
            force_domains: Vec::new(),
            system: SystemConfig::default(),
            logs: LogsConfig::default(),
            auth: AuthConfig::default(),
            notifications: NotificationsConfig::default(),
            provider_aliases: std::collections::BTreeMap::new(),
            antigravity: AntigravityConfig::default(),
            flow_server: None,
            adblock_enabled: false,
            schedules: Vec::new(),
            zapret: ZapretConfig::default(),
            gaming: GamingConfig::default(),
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

    pub fn mihomo_proxy_url(&self) -> String {
        format!("http://{}:{}", self.mihomo.host, self.mihomo.mixed_port)
    }

    pub fn health_check_url(&self) -> &str {
        let u = self.mihomo.health_check_url.trim();
        if u.is_empty() {
            "http://www.gstatic.com/generate_204"
        } else {
            u
        }
    }

    pub fn health_check_url_encoded(&self) -> String {
        crate::mihomo::urlencoding_lite(self.health_check_url())
    }
}

/// Рекурсивный merge: значения из `over` поверх `base` (объекты мержатся, остальное заменяется).
pub fn merge_value(base: &mut serde_json::Value, over: &serde_json::Value) {
    match (base, over) {
        (serde_json::Value::Object(base_map), serde_json::Value::Object(over_map)) => {
            for (k, v) in over_map {
                // Если значение замаскировано звездочками, не перезаписываем существующий секрет
                if v.as_str() == Some("******") {
                    continue;
                }
                match base_map.get_mut(k) {
                    Some(slot) => merge_value(slot, v),
                    None => {
                        base_map.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (slot, over) => {
            if over.as_str() != Some("******") {
                *slot = over.clone();
            }
        }
    }
}

pub(crate) fn parse_config_content(content: &str, path_display: &str) -> AppConfig {
    let mut base = serde_json::to_value(AppConfig::default()).unwrap_or_default();
    match serde_json::from_str::<serde_json::Value>(content) {
        Ok(over) => merge_value(&mut base, &over),
        Err(e) => eprintln!("[WARN] {} не JSON: {} — использую дефолты", path_display, e),
    }
    serde_json::from_value::<AppConfig>(base)
        .map(|mut c| {
            c.failover.migrate_priority();
            c
        })
        .unwrap_or_else(|e| {
            eprintln!("[WARN] Ошибка конфига {}: {} — использую дефолты", path_display, e);
            AppConfig::default()
        })
}

/// Синхронная загрузка конфига с deep-merge поверх дефолтов (для инициализации CLI/main).
pub fn load(path: &Path) -> AppConfig {
    match std::fs::read_to_string(path) {
        Ok(content) => parse_config_content(&content, &path.display().to_string()),
        Err(_) => AppConfig::default(),
    }
}

/// Асинхронная загрузка конфига (не блокирует воркеры Tokio в обработчиках API).
pub async fn load_async(path: &Path) -> AppConfig {
    match tokio::fs::read_to_string(path).await {
        Ok(content) => parse_config_content(&content, &path.display().to_string()),
        Err(_) => AppConfig::default(),
    }
}

/// Сохранение конфига атомарно (tmp + rename).
pub async fn save(path: &Path, cfg: &AppConfig) -> Result<(), String> {
    let serialized = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let _ = tokio::fs::create_dir_all(parent).await;

    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp_name = format!("config.{}.{}.tmp", std::process::id(), nonce);
    let tmp = parent.join(tmp_name);

    if let Err(e) = tokio::fs::write(&tmp, serialized).await {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(e.to_string());
    }
    if let Err(e) = tokio::fs::rename(&tmp, path).await {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(e.to_string());
    }
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

    #[test]
    fn load_broken_json_returns_default() {
        let path = std::env::temp_dir().join("xr-test-broken.json");
        std::fs::write(&path, "{ invalid json structure ...").unwrap();
        let cfg = load(&path);
        assert_eq!(cfg.refresh_interval_sec, 10);
        assert_eq!(cfg.rci.port, 79);
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn load_async_roundtrip() {
        let path = std::env::temp_dir().join("xr-test-async-cfg.json");
        let mut cfg = AppConfig::default();
        cfg.refresh_interval_sec = 42;
        save(&path, &cfg).await.unwrap();
        let loaded = load_async(&path).await;
        assert_eq!(loaded.refresh_interval_sec, 42);
        let _ = tokio::fs::remove_file(&path).await;
    }

    #[test]
    fn test_normalize_domain() {
        assert_eq!(normalize_domain("https://habr.com/"), "habr.com");
        assert_eq!(normalize_domain("HTTPS://MYSKU.CLUB:443"), "mysku.club");
        assert_eq!(
            normalize_domain("https://sub.domain.co.uk:8080/path?query=1#hash"),
            "sub.domain.co.uk"
        );
        assert_eq!(normalize_domain("WWW.EXAMPLE.COM."), "example.com");
        assert_eq!(normalize_domain("user:pass@example.com"), "example.com");
        assert_eq!(normalize_domain("  twitch.tv  "), "twitch.tv");
    }

    #[test]
    fn test_is_valid_domain() {
        assert!(is_valid_domain("mysku.club"));
        assert!(is_valid_domain("habr.com"));
        assert!(is_valid_domain("sub.domain.co.uk"));
        assert!(!is_valid_domain("localhost"));
        assert!(!is_valid_domain("192.168.1.1"));
        assert!(!is_valid_domain("127.0.0.1"));
        assert!(!is_valid_domain("bad;domain"));
        assert!(!is_valid_domain(""));
        assert!(!is_valid_domain("domain_without_dot"));
    }
}
