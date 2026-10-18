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
    /// Путь к исполняемому файлу xkeen-route.
    pub xkeen_route_bin: String,
    /// Init-скрипт службы xkeen-route.
    pub xkeen_route_init: String,
    /// Путь к исполняемому файлу mihomo.
    pub mihomo_bin: String,
    /// Init-скрипт службы zapret.
    pub zapret_init: String,
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
            xkeen_route_bin: "/opt/sbin/xkeen-route".into(),
            xkeen_route_init: "/opt/etc/init.d/S99xkeen-route".into(),
            mihomo_bin: "/opt/sbin/mihomo".into(),
            zapret_init: "/opt/etc/init.d/S51zapret".into(),
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

fn default_best_ping_pool_size() -> usize {
    15
}

fn default_best_ping_hysteresis_ms() -> u32 {
    40
}

fn default_canary_duration_secs() -> u64 {
    60
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
    /// Длительность теста стабильности (Canary Soak Test) перед возвратом на приоритетный сервер (сек, по умолчанию 60)
    #[serde(default = "default_canary_duration_secs")]
    pub canary_duration_secs: u64,
    /// Per-device failover: мониторинг цепочек server+резервы (device_routing).
    pub device_failover_enabled: bool,
    /// Именованные слоты настроенных пресетов для мгновенного переключения
    pub presets: Vec<FailoverPresetSlot>,
    /// Динамический выбор лучшего по пингу (Best-Ping Failover)
    #[serde(default)]
    pub best_ping_enabled: bool,
    /// Размер пула серверов для Best-Ping проверки (по умолчанию 15)
    #[serde(default = "default_best_ping_pool_size")]
    pub best_ping_pool_size: usize,
    /// Гистерезис пинга в миллисекундах (30-50 мс, по умолчанию 40)
    #[serde(default = "default_best_ping_hysteresis_ms")]
    pub best_ping_hysteresis_ms: u32,
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
            canary_duration_secs: 60,
            device_failover_enabled: false,
            presets: Vec::new(),
            best_ping_enabled: false,
            best_ping_pool_size: 15,
            best_ping_hysteresis_ms: 40,
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
    #[serde(default = "default_true")]
    pub follow_whitelist: bool,
}

impl Default for DeviceRouting {
    fn default() -> Self {
        Self {
            servers: Vec::new(),
            ping_threshold_ms: 300,
            auto_restore: true,
            follow_whitelist: true,
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
#[derive(Default)]
pub struct AuthConfig {
    pub enabled: bool,
    pub password_hash: String,
    pub salt: String,
    pub session_secret: String,
}


#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
#[derive(Default)]
pub struct NotificationsConfig {
    pub telegram_enabled: bool,
    pub telegram_bot_token: String,
    pub telegram_chat_id: String,
    pub webhook_url: String,
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
    /// Отключенные подписки (провайдеры): их серверы не используются и скрываются из списков.
    pub disabled_providers: Vec<String>,
    /// Автоматический подбор сервера для Google AI / Flow.
    pub flow_auto_select: bool,
    /// Блокировка рекламы на роутере через Mihomo (GEOSITE,category-ads-all,REJECT).
    pub adblock_enabled: bool,
    /// Расписания работы устройств (блокировка/прокси/direct по часам).
    pub schedules: Vec<DeviceSchedule>,
    /// Умные режимы и гибридная маршрутизация Zapret DPI.
    pub zapret: ZapretConfig,
    /// Отдельный модуль игрового режима (обход игровых блокировок).
    pub gaming: GamingConfig,
    /// Полноценная интеграция AdGuard Home.
    pub adguard: AdGuardConfig,
    /// Пользовательские названия серверов (server_id -> alias).
    #[serde(default)]
    pub server_aliases: std::collections::HashMap<String, String>,
    /// Серверы, отмеченные для обхода белых списков.
    #[serde(default)]
    pub whitelist_servers: Vec<String>,
    /// Подписки (провайдеры), целиком отмеченные для обхода белых списков.
    #[serde(default)]
    pub whitelist_providers: Vec<String>,
    /// Настройки выделенного режима белых списков (376 domestic domains direct, proxy tunnel).
    #[serde(default)]
    pub whitelist: WhitelistConfig,
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
#[derive(Default)]
pub enum GamingMode {
    #[default]
    Compatibility,
    BypassRu,
    SmartSplit,
    KnownServices,
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
    #[serde(default)]
    pub server: Option<String>,
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

fn default_smart_idle_mins() -> u32 {
    60
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
    /// Умный игровой режим: авто-включение при обнаружении соединений к игровым серверам
    #[serde(default)]
    pub smart_mode: bool,
    /// Таймаут бездействия в минутах до авто-отключения (по умолчанию 60 минут)
    #[serde(default = "default_smart_idle_mins")]
    pub smart_idle_timeout_mins: u32,
    /// Список игнорируемых игровых соединений ("Не реагировать")
    #[serde(default)]
    pub ignored_game_conns: Vec<String>,
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
            smart_mode: false,
            smart_idle_timeout_mins: 60,
            ignored_game_conns: Vec::new(),
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

fn default_true() -> bool {
    true
}

fn default_agh_host() -> String {
    "127.0.0.1".to_string()
}

fn default_agh_http_port() -> u16 {
    3000
}

fn default_agh_dns_port() -> u16 {
    5354
}

fn default_agh_mode() -> String {
    "managed".to_string()
}

fn default_agh_upstream() -> Vec<String> {
    vec![
        "127.0.0.1:1053".to_string(),
        "77.88.8.8".to_string(),
    ]
}

fn default_agh_config_path() -> String {
    "/opt/etc/AdGuardHome.yaml".to_string()
}

/// Настройки интеграции полнофункционального AdGuard Home.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct AdGuardConfig {
    pub enabled: bool,
    #[serde(default = "default_agh_host")]
    pub host: String,
    #[serde(default = "default_agh_http_port")]
    pub http_port: u16,
    #[serde(default = "default_agh_dns_port")]
    pub dns_port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
    #[serde(default = "default_agh_mode")]
    pub integration_mode: String, // "managed" | "external"
    #[serde(default = "default_agh_upstream")]
    pub upstream_dns: Vec<String>,
    /// Защитный механизм отката транзакции при потере DNS
    #[serde(default = "default_true")]
    pub failsafe_rollback: bool,
    /// Путь к файлу конфигурации AdGuardHome.yaml (для managed-режима и бэкапов)
    #[serde(default = "default_agh_config_path")]
    pub config_path: String,
}

impl Default for AdGuardConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            host: default_agh_host(),
            http_port: default_agh_http_port(),
            dns_port: default_agh_dns_port(),
            username: None,
            password: None,
            integration_mode: default_agh_mode(),
            upstream_dns: default_agh_upstream(),
            failsafe_rollback: true,
            config_path: default_agh_config_path(),
        }
    }
}

/// Устройство в режиме белых списков (100% туннель + отечественные сервисы напрямую).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(default)]
pub struct WhitelistDevice {
    pub mac: String,
    pub ip: String,
    pub enabled: bool,
    #[serde(default)]
    pub name: Option<String>,
}

impl Default for WhitelistDevice {
    fn default() -> Self {
        Self {
            mac: String::new(),
            ip: String::new(),
            enabled: true,
            name: None,
        }
    }
}

/// Стратегия маршрутизации при активном сервере белых списков
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WhitelistStrategy {
    /// Специальный режим Белых списков XKeen Route: 100% трафик устройств в туннель, 376 отечественных сайтов DIRECT
    StrictXKeen,
    /// Стандартная маршрутизация сервера (обычные правила и проксирование)
    Standard,
}

impl Default for WhitelistStrategy {
    fn default() -> Self {
        Self::StrictXKeen
    }
}

/// Настройки выделенного режима белых списков (Белые списки: 376 отечественных доменов напрямую, прокси-туннель).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(default)]
pub struct WhitelistConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub auto_by_failover: bool,
    #[serde(default)]
    pub strategy: WhitelistStrategy,
    #[serde(default)]
    pub target_server: Option<String>,
    #[serde(default = "default_true")]
    pub follow_failover: bool,
    #[serde(default)]
    pub excluded_devices: Vec<String>,
    #[serde(default)]
    pub devices: Vec<WhitelistDevice>,
    #[serde(default)]
    pub gaming_suspended: bool,
    #[serde(default)]
    pub custom_domains: Vec<String>,
}

impl Default for WhitelistConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            auto_by_failover: true,
            strategy: WhitelistStrategy::StrictXKeen,
            target_server: None,
            follow_failover: true,
            excluded_devices: Vec::new(),
            devices: Vec::new(),
            gaming_suspended: false,
            custom_domains: Vec::new(),
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
    let no_user = host_part.split('@').next_back().unwrap_or(host_part);
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

/// Проверяет, является ли строка корректным IPv4/IPv6 адресом или CIDR нотацией (например, 192.168.1.1, 10.0.0.0/24, 2001:db8::1/128).
pub fn is_valid_ip_or_cidr(s: &str) -> bool {
    let clean = s.trim();
    if clean.is_empty() {
        return false;
    }
    if clean.parse::<std::net::IpAddr>().is_ok() {
        return true;
    }
    if let Some((addr_str, prefix_str)) = clean.split_once('/') {
        if let Ok(addr) = addr_str.trim().parse::<std::net::IpAddr>() {
            if let Ok(prefix) = prefix_str.trim().parse::<u8>() {
                if (addr.is_ipv4() && prefix <= 32) || (addr.is_ipv6() && prefix <= 128) {
                    return true;
                }
            }
        }
    }
    false
}

/// Проверяет, является ли строка корректным аппаратным MAC-адресом (например, "AA:BB:CC:DD:EE:FF" или "aa-bb-cc-dd-ee-ff").
pub fn is_valid_mac(s: &str) -> bool {
    let clean = s.trim();
    let parts: Vec<&str> = if clean.contains(':') {
        clean.split(':').collect()
    } else if clean.contains('-') {
        clean.split('-').collect()
    } else {
        return false;
    };
    if parts.len() != 6 {
        return false;
    }
    parts.iter().all(|part| part.len() == 2 && part.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Строгая проверка идентификатора провайдера/подписки для предотвращения Path Traversal и инъекций.
/// Разрешены только символы [a-zA-Z0-9_-], длина 1..=64, запрещены '..', '/', '\'.
pub fn is_valid_provider_id(id: &str) -> bool {
    let s = id.trim();
    if s.is_empty() || s.len() > 64 {
        return false;
    }
    if s.contains("..") || s.contains('/') || s.contains('\\') {
        return false;
    }
    s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
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
    /// Специальный профиль «Smart TV / Кинотеатр» (блокировка UDP 443 + оптимизация GGC)
    pub smart_tv_mode: bool,
    /// Включено ли автообновление списков (Community Hostlists)
    pub community_hostlist_enabled: bool,
    /// URL внешнего списка (community antizapret / custom hostlist)
    pub community_hostlist_url: String,
    /// Автоматическое обновление списка
    pub community_hostlist_auto_update: bool,
    /// Дата и время последнего обновления списка
    pub community_hostlist_last_updated: Option<String>,
    /// Количество доменов в community hostlist
    pub community_hostlist_count: usize,
    /// Исключенные устройства из перехвата Zapret (по IP или MAC): для них Zapret выключен
    pub excluded_devices: Vec<String>,
    /// Выбранный движок DPI: "v2" (Modern Zapret 2.0 nfqws2 + Lua) или "v1" (Legacy Zapret 1.x nfqws)
    pub engine: String,
    /// Отметка времени последней проверки обновлений Zapret (5:00 утра раз в сутки)
    #[serde(default)]
    pub last_update_check: Option<String>,
    /// Доступно ли обновление для Zapret
    #[serde(default)]
    pub update_available: bool,
    /// Последняя доступная версия Zapret
    #[serde(default)]
    pub latest_version: Option<String>,
    /// Накопленный спасённый трафик на VPS за период (байты)
    #[serde(default)]
    pub period_saved_bytes: u64,
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
            smart_tv_mode: false,
            community_hostlist_enabled: false,
            community_hostlist_url: "https://raw.githubusercontent.com/zapret-info/z-block/master/hosts.txt".to_string(),
            community_hostlist_auto_update: false,
            community_hostlist_last_updated: None,
            community_hostlist_count: 0,
            excluded_devices: Vec::new(),
            engine: "v2".to_string(),
            last_update_check: None,
            update_available: false,
            latest_version: None,
            period_saved_bytes: 0,
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
            disabled_providers: Vec::new(),
            flow_auto_select: false,
            adblock_enabled: false,
            schedules: Vec::new(),
            zapret: ZapretConfig::default(),
            gaming: GamingConfig::default(),
            adguard: AdGuardConfig::default(),
            server_aliases: std::collections::HashMap::new(),
            whitelist_servers: Vec::new(),
            whitelist_providers: Vec::new(),
            whitelist: WhitelistConfig::default(),
        }
    }
}

impl AppConfig {
    pub fn adguard_url(&self) -> String {
        format!("http://{}:{}", self.adguard.host, self.adguard.http_port)
    }

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

/// Строгий парсинг конфига. Возвращает Err, если JSON некорректен или структура невалидна.
pub fn parse_config_strict(content: &str) -> Result<AppConfig, String> {
    let over: serde_json::Value = serde_json::from_str(content)
        .map_err(|e| format!("Некорректный JSON конфигурации: {e}"))?;
    let mut base = serde_json::to_value(AppConfig::default()).unwrap_or_default();
    merge_value(&mut base, &over);
    let mut cfg: AppConfig = serde_json::from_value(base)
        .map_err(|e| format!("Некорректная структура конфигурации: {e}"))?;
    cfg.failover.migrate_priority();
    Ok(cfg)
}

/// Проверка безопасности пути к init-скрипту (разрешены только пути в /opt/ и символы [a-zA-Z0-9_\-\./])
pub fn is_valid_init_script_path(path: &str) -> bool {
    let p = path.trim();
    if !p.starts_with("/opt/") || p.contains("..") {
        return false;
    }
    !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.' || c == '/')
}

/// Извлечение строкового поля из синтаксически поврежденного JSON через поиск подстроки
pub fn extract_string_field_fallback(content: &str, field_name: &str) -> Option<String> {
    let key = format!("\"{field_name}\"");
    let key_pos = content.find(&key)?;
    let after_key = &content[key_pos + key.len()..];
    let colon_pos = after_key.find(':')?;
    let after_colon = &after_key[colon_pos + 1..];
    let quote_start = after_colon.find('"')?;
    let after_start_quote = &after_colon[quote_start + 1..];
    let quote_end = after_start_quote.find('"')?;
    let val = &after_start_quote[..quote_end];
    if !val.is_empty() {
        Some(val.to_string())
    } else {
        None
    }
}

pub(crate) fn parse_config_content(content: &str, path_display: &str) -> AppConfig {
    match parse_config_strict(content) {
        Ok(c) => c,
        Err(err) => {
            eprintln!("[ERROR] {}: {err}", path_display);
            // Пытаемся восстановить секцию auth, чтобы ни при каких условиях не отключить авторизацию
            let mut fallback = AppConfig::default();
            let mut preserved_auth = false;

            if let Ok(over) = serde_json::from_str::<serde_json::Value>(content) {
                if let Some(auth_val) = over.get("auth") {
                    if let Ok(auth) = serde_json::from_value::<AuthConfig>(auth_val.clone()) {
                        fallback.auth = auth;
                        preserved_auth = true;
                    }
                }
            }

            // Если JSON был поврежден на уровне синтаксиса, но содержал следы включенной auth
            if !preserved_auth && (content.contains("\"password_hash\"") || content.contains("\"auth\"")) {
                eprintln!("[SECURITY] Обнаружены следы авторизации в поврежденном конфиге! Сохраняем защитный режим auth.");
                fallback.auth.enabled = true;
                if let Some(hash) = extract_string_field_fallback(content, "password_hash") {
                    fallback.auth.password_hash = hash;
                }
                if let Some(salt) = extract_string_field_fallback(content, "salt") {
                    fallback.auth.salt = salt;
                }
                if let Some(secret) = extract_string_field_fallback(content, "session_secret") {
                    fallback.auth.session_secret = secret;
                }
            }

            fallback
        }
    }
}

/// Синхронная загрузка конфига с deep-merge поверх дефолтов (для инициализации CLI/main).
pub fn load(path: &Path) -> AppConfig {
    if !path.exists() {
        return AppConfig::default();
    }
    match std::fs::read_to_string(path) {
        Ok(content) => {
            match parse_config_strict(&content) {
                Ok(c) => c,
                Err(err) => {
                    eprintln!("[ERROR] Поврежденный файл конфигурации {}: {err}", path.display());
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    let backup_path = path.with_extension(format!("corrupted.{ts}.bak"));
                    let _ = std::fs::copy(path, &backup_path);
                    eprintln!("[INFO] Создан бэкап поврежденного конфига: {}", backup_path.display());
                    parse_config_content(&content, &path.display().to_string())
                }
            }
        }
        Err(e) => {
            eprintln!("[ERROR] Не удалось прочитать существующий конфиг {}: {e}", path.display());
            let mut cfg = AppConfig::default();
            cfg.auth.enabled = true;
            cfg
        }
    }
}

/// Асинхронная загрузка конфига (не блокирует воркеры Tokio в обработчиках API).
pub async fn load_async(path: &Path) -> AppConfig {
    match tokio::fs::read_to_string(path).await {
        Ok(content) => {
            match parse_config_strict(&content) {
                Ok(c) => c,
                Err(err) => {
                    eprintln!("[ERROR] Поврежденный файл конфигурации {}: {err}", path.display());
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    let backup_path = path.with_extension(format!("corrupted.{ts}.bak"));
                    let _ = tokio::fs::copy(path, &backup_path).await;
                    parse_config_content(&content, &path.display().to_string())
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            AppConfig::default()
        }
        Err(e) => {
            eprintln!("[ERROR] Не удалось прочитать существующий конфиг {}: {e}", path.display());
            let mut cfg = AppConfig::default();
            cfg.auth.enabled = true;
            cfg
        }
    }
}

/// Сохранение конфига атомарно (tmp + rename).
pub async fn save(path: &Path, cfg: &AppConfig) -> Result<(), String> {
    // BUG-11: Блокировка стирания пароля при включенной авторизации
    if cfg.auth.enabled && cfg.auth.password_hash.trim().is_empty() {
        return Err("Отказ сохранения: попытка сохранить включенную авторизацию с пустым хэшем пароля".to_string());
    }

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
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!("xr-test-config-{}-{}.json", std::process::id(), nonce));
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
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!("xr-test-config2-{}-{}.json", std::process::id(), nonce));
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
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!("xr-test-broken-{}-{}.json", std::process::id(), nonce));
        std::fs::write(&path, "{ invalid json structure ...").unwrap();
        let cfg = load(&path);
        assert_eq!(cfg.refresh_interval_sec, 10);
        assert_eq!(cfg.rci.port, 79);
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn load_async_roundtrip() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!("xr-test-async-cfg-{}-{}.json", std::process::id(), nonce));
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

    #[test]
    fn test_corrupted_config_preserves_auth() {
        let path = std::env::temp_dir().join("xr-test-corrupted-auth.json");
        std::fs::write(&path, r#"{"auth": {"enabled": true, "password_hash": "$pbkdf2$hash", "salt": "saltsalt"}, "mihomo": { corrupted"#).unwrap();
        let cfg = load(&path);
        assert!(cfg.auth.enabled, "Auth must remain enabled even if config JSON is corrupted");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_invalid_structure_preserves_auth_object() {
        let path = std::env::temp_dir().join("xr-test-invalid-struct.json");
        std::fs::write(&path, r#"{"auth": {"enabled": true, "password_hash": "myhash", "salt": "mysalt", "session_secret": "sec"}, "rci": {"port": "not-a-number"}}"#).unwrap();
        let cfg = load(&path);
        assert!(cfg.auth.enabled);
        assert_eq!(cfg.auth.password_hash, "myhash");
        assert_eq!(cfg.auth.salt, "mysalt");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_strict_config_parsing_fails_on_corrupt_json() {
        assert!(parse_config_strict("not json at all").is_err());
        assert!(parse_config_strict(r#"{"failover": {"enabled": true}}"#).is_ok());
    }

    #[test]
    fn test_is_valid_ip_or_cidr() {
        assert!(is_valid_ip_or_cidr("192.168.1.1"));
        assert!(is_valid_ip_or_cidr("10.0.0.1/24"));
        assert!(is_valid_ip_or_cidr("192.168.1.50/32"));
        assert!(is_valid_ip_or_cidr("::1"));
        assert!(is_valid_ip_or_cidr("2001:db8::1/128"));
        assert!(is_valid_ip_or_cidr("2001:db8::/64"));
        assert!(!is_valid_ip_or_cidr(""));
        assert!(!is_valid_ip_or_cidr("not-an-ip"));
        assert!(!is_valid_ip_or_cidr("192.168.1.50/33"));
        assert!(!is_valid_ip_or_cidr("2001:db8::1/129"));
        assert!(!is_valid_ip_or_cidr("192.168.1.50/32/32"));
    }

    #[test]
    fn test_is_valid_mac() {
        assert!(is_valid_mac("AA:BB:CC:DD:EE:FF"));
        assert!(is_valid_mac("aa:bb:cc:dd:ee:ff"));
        assert!(is_valid_mac("00:11:22:33:44:55"));
        assert!(is_valid_mac("00-11-22-33-44-55"));
        assert!(!is_valid_mac(""));
        assert!(!is_valid_mac("00:11:22:33:44"));
        assert!(!is_valid_mac("00:11:22:33:44:55:66"));
        assert!(!is_valid_mac("00:11:22:33:44:ZZ"));
        assert!(!is_valid_mac("00:11:22:33:44:55; rm -rf /"));
        assert!(!is_valid_mac("192.168.1.1"));
    }

    #[test]
    fn test_is_valid_provider_id() {
        assert!(is_valid_provider_id("provider_1"));
        assert!(is_valid_provider_id("custom-sub"));
        assert!(is_valid_provider_id("subscription123"));
        assert!(is_valid_provider_id("ABC_xyz-09"));
        assert!(!is_valid_provider_id(""));
        assert!(!is_valid_provider_id("../etc/passwd"));
        assert!(!is_valid_provider_id("..\\etc\\passwd"));
        assert!(!is_valid_provider_id("prov/sub"));
        assert!(!is_valid_provider_id("prov\\sub"));
        assert!(!is_valid_provider_id("prov sub"));
        assert!(!is_valid_provider_id("prov;rm"));
        assert!(!is_valid_provider_id(&"a".repeat(65)));
    }

    #[test]
    fn test_is_valid_init_script_path() {
        // SEC-05: Path traversal and prefix validation
        assert!(is_valid_init_script_path("/opt/etc/init.d/S51zapret"));
        assert!(is_valid_init_script_path("/opt/bin/xkeen"));
        assert!(is_valid_init_script_path("/opt/sbin/service-run"));

        // Reject non-/opt/, path traversal, and malicious characters
        assert!(!is_valid_init_script_path("/etc/init.d/S51zapret"));
        assert!(!is_valid_init_script_path("/opt/etc/init.d/../../etc/passwd"));
        assert!(!is_valid_init_script_path("/opt/etc/init.d/..\\..\\windows\\system32"));
        assert!(!is_valid_init_script_path("/opt/bin/script.sh; rm -rf /"));
        assert!(!is_valid_init_script_path("/opt/bin/script.sh | sh"));
        assert!(!is_valid_init_script_path(""));
    }

    #[test]
    fn test_corrupted_auth_recovery_from_json() {
        // BUG-11: Fallback regex recovery of auth credentials from corrupted JSON
        let corrupted_json = r#"{
            "auth": {
                "enabled": true,
                "password_hash": "a1b2c3d4e5f67890",
                "salt": "supersalt123",
                "session_secret": "secret999"
            },
            "broken_trailing_comma":,
        }"#;

        let parsed = parse_config_content(corrupted_json, "test_config.json");
        assert!(parsed.auth.enabled, "Auth should be kept enabled when credentials exist");
        assert_eq!(parsed.auth.password_hash, "a1b2c3d4e5f67890");
        assert_eq!(parsed.auth.salt, "supersalt123");
        assert_eq!(parsed.auth.session_secret, "secret999");
    }

    #[tokio::test]
    async fn test_save_blank_password_prevention() {
        // BUG-11: Prevent wiping password hash when auth is enabled
        let mut cfg = AppConfig::default();
        cfg.auth.enabled = true;
        cfg.auth.password_hash = "   ".to_string();

        let tmp_file = std::env::temp_dir().join("xkeen_test_save_blank_pw.json");
        let res = save(&tmp_file, &cfg).await;
        assert!(res.is_err(), "Saving config with enabled auth but blank password hash must be rejected");
    }

    #[test]
    fn test_whitelist_providers_serialization_and_default() {
        let json_empty = r#"{}"#;
        let parsed = parse_config_content(json_empty, "test_empty.json");
        assert!(parsed.whitelist_providers.is_empty());

        let json_with_providers = r#"{
            "whitelist_providers": ["sub_alpha", "sub_beta"]
        }"#;
        let parsed2 = parse_config_content(json_with_providers, "test_prov.json");
        assert_eq!(parsed2.whitelist_providers, vec!["sub_alpha", "sub_beta"]);
    }
}
