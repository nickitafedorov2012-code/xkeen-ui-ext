mod api;
mod config;
mod failover;
mod frontend;
mod logger;
mod mihomo;
mod rci;
mod routing;
pub mod transaction;
mod updater;
mod override_sync;
mod cdn_discovery;
mod antigravity;
mod auth;
mod watchdog;
mod speedtest;
mod notifications;
pub mod traffic;
mod system;

use axum::extract::Request;
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{any, delete, get, post, put};
use axum::Router;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;


pub const VERSION: &str = match option_env!("XKEEN_ROUTE_VERSION") {
    Some(v) => v,
    None => concat!("v", env!("CARGO_PKG_VERSION")),
};
pub const APP_NAME: &str = "XKeen Route";

#[cfg(target_os = "linux")]
pub const CONFIG_PATH: &str = "/opt/etc/xkeen-route/config.json";
#[cfg(not(target_os = "linux"))]
pub const CONFIG_PATH: &str = "xkeen-route.config.json";

#[cfg(target_os = "linux")]
pub const INIT_SCRIPT: &str = "/opt/etc/init.d/S99xkeen-route";
#[cfg(not(target_os = "linux"))]
pub const INIT_SCRIPT: &str = "S99xkeen-route";

#[derive(Parser)]
#[command(
    name = "xkeen-route",
    about = "XKeen Route — веб-панель управления маршрутизацией Keenetic/Mihomo",
    disable_version_flag = true,
    disable_help_subcommand = true
)]
struct Cli {
    /// Хост веб-панели (по умолчанию 0.0.0.0)
    #[arg(short = 'H', long = "host", default_value = "0.0.0.0")]
    host: String,

    /// Порт веб-панели
    #[arg(short = 'p', long = "port", default_value = "1001")]
    port: u16,

    /// Путь к конфигу (по умолчанию /opt/etc/xkeen-route/config.json)
    #[arg(short = 'c', long = "config")]
    config: Option<String>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Создать init-скрипт для Entware
    CreateInit,
    /// Показать версию
    Version,
    /// Сбросить пароль панели (отключить авторизацию)
    ResetPassword,
    /// Установить новый пароль панели
    SetPassword {
        /// Новый пароль
        password: String,
    },
}

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RwLock<Arc<config::AppConfig>>>,
    pub config_path: Arc<PathBuf>,
    pub http: reqwest::Client,
    /// Лента событий failover.
    pub failover_log: Arc<failover::FailoverLog>,
    /// Сериализация правок config.yaml (гонки failover/ручных правок).
    pub routing_lock: Arc<tokio::sync::Mutex<()>>,
    /// Сериализация read-modify-write конфига панели (config.json) — против TOCTOU.
    pub config_lock: Arc<tokio::sync::Mutex<()>>,
    /// Сериализация speedtest для исключения гонок при переключении узлов.
    pub speedtest_lock: Arc<tokio::sync::Mutex<()>>,
    /// Менеджер обхода блокировок Google Antigravity.
    pub antigravity: Arc<antigravity::AntigravityManager>,
}

fn create_init(port: u16) -> std::io::Result<()> {
    if let Some(dir) = PathBuf::from(INIT_SCRIPT).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let content = format!(
        r#"#!/bin/sh

ENABLED=yes
PROCS=xkeen-route
ARGS="-p {port}"
PREARGS=""
DESC=$PROCS
PATH=/opt/sbin:/opt/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

. /opt/etc/init.d/rc.func
"#
    );
    std::fs::write(INIT_SCRIPT, content)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(INIT_SCRIPT, std::fs::Permissions::from_mode(0o755))?;
        // Добавление сторожевого таймера (watchdog) в crontab для автоперезапуска
        let _ = std::process::Command::new("sh")
            .arg("-c")
            .arg("(crontab -l 2>/dev/null | grep -v 'xkeen-route'; echo '*/5 * * * * pidof xkeen-route >/dev/null || /opt/etc/init.d/S99xkeen-route start >/dev/null 2>&1') | crontab -")
            .status();
    }
    println!("[OK] Init-скрипт создан: {}", INIT_SCRIPT);
    Ok(())
}

/// Запрет кэширования ответов панели (иначе браузер показывает устаревшие данные).
async fn no_cache(req: Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    if let Ok(v) = axum::http::HeaderValue::from_str("no-store") {
        res.headers_mut().insert("Cache-Control", v);
    }
    res
}

/// Логирование HTTP-запросов: метод, путь, статус, длительность.
async fn log_requests(req: Request, next: Next) -> Response {
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let query = req.uri().query().unwrap_or("").to_string();
    let start = std::time::Instant::now();
    let res = next.run(req).await;
    // Не логируем частые опросы статуса — шум.
    if path != "/api/status" && path != "/api/system/metrics" && path != "/api/system/processes" {
        log_i!(
            "{} {}{} -> {} ({} мс)",
            method,
            path,
            if query.is_empty() { String::new() } else { format!("?{}", query) },
            res.status().as_u16(),
            start.elapsed().as_millis()
        );
    }
    res
}

/// Ожидание готовности RCI роутера при старте (после перезагрузки роутера).
async fn wait_for_router_ready(state: &AppState) {
    let max_attempts = 30; // 30 * 2 сек = 60 сек максимум
    for attempt in 1..=max_attempts {
        let cfg = state.config.read().await.clone();
        match rci::get_version(&state.http, &cfg).await {
            Ok(_) => {
                log_i!("RCI роутера доступен (попытка {})", attempt);
                return;
            }
            Err(_) => {
                if attempt == 1 {
                    log_i!("Ожидание готовности RCI роутера после старта...");
                }
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        }
    }
    log_w!("RCI роутера не ответил за 60 сек, продолжаем в автономном режиме");
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let config_path = PathBuf::from(cli.config.clone().unwrap_or_else(|| CONFIG_PATH.to_string()));

    match cli.command {
        Some(Command::Version) => {
            println!("{} {}", APP_NAME, VERSION);
            return;
        }
        Some(Command::CreateInit) => {
            if let Err(e) = create_init(cli.port) {
                eprintln!("[ERROR] {}", e);
                std::process::exit(1);
            }
            return;
        }
        Some(Command::ResetPassword) => {
            if let Err(e) = auth::cli_reset_password(&config_path) {
                eprintln!("[ERROR] Не удалось сбросить пароль: {}", e);
                std::process::exit(1);
            }
            return;
        }
        Some(Command::SetPassword { password }) => {
            if let Err(e) = auth::cli_set_password(&config_path, &password) {
                eprintln!("[ERROR] Не удалось установить пароль: {}", e);
                std::process::exit(1);
            }
            return;
        }
        None => {}
    }

    let cfg = config::load(&config_path);
    logger::init(&config_path, &cfg.logs.remote_syslog);
    logger::set_level(&cfg.logs.level);
    log_i!("{} {} запущен, конфиг: {}", APP_NAME, VERSION, config_path.display());
    if !cfg.logs.remote_syslog.is_empty() {
        log_i!("Логи дублируются на syslog {}", cfg.logs.remote_syslog);
    }

    let port = cli.port;
    let config = Arc::new(RwLock::new(Arc::new(cfg)));
    let ag_mgr = Arc::new(antigravity::AntigravityManager::new(Arc::clone(&config)));

    {
        let c = config.read().await;
        if c.antigravity.enabled && c.antigravity.proxy_enabled {
            ag_mgr.clone().start_proxy(c.antigravity.proxy_port);
        }
        ag_mgr.clone().start_warm_loop();
    }

    let state = AppState {
        config,
        config_path: Arc::new(config_path),
        http: match reqwest::Client::builder()
            .cookie_store(true)
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                log_e!("Не удалось инициализировать HTTP клиент: {e}");
                std::process::exit(1);
            }
        },
        failover_log: Arc::new(failover::FailoverLog::default()),
        routing_lock: Arc::new(tokio::sync::Mutex::new(())),
        config_lock: Arc::new(tokio::sync::Mutex::new(())),
        speedtest_lock: Arc::new(tokio::sync::Mutex::new(())),
        antigravity: ag_mgr,
    };

    failover::spawn(state.clone());
    watchdog::spawn(state.clone());
    traffic::spawn(state.clone());

    // Начальная быстрая синхронизация известных бандлов IP принудительно проксируемых доменов с geo_override
    let sync_state = state.clone();
    tokio::spawn(async move {
        // Задержка и ожидание готовности роутера/DNS после перезагрузки
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        wait_for_router_ready(&sync_state).await;

        let cfg = sync_state.config.read().await;
        if !cfg.force_domains.is_empty() {
            log_i!("[STARTUP] Быстрая синхронизация известных CDN бандлов с geo_override (без сетевого сканирования)...");
            let static_cdns = cdn_discovery::expand_bundles(&cfg.force_domains);
            let mut all_domains = cfg.force_domains.clone();
            all_domains.extend(static_cdns);
            if let Err(e) = override_sync::sync_geo_override(&all_domains).await {
                log_w!("[STARTUP] Ошибка быстрой синхронизации geo_override: {}", e);
            } else {
                log_i!("[STARTUP] ✓ Быстрая синхронизация geo_override завершена");
            }
        }
    });

    // Еженедельное плановое сканирование CDN (каждый понедельник в 05:00 утра)
    let weekly_state = state.clone();
    tokio::spawn(async move {
        loop {
            let wait_dur = duration_until_next_monday_5am();
            let next_run = chrono::Local::now() + chrono::Duration::from_std(wait_dur).unwrap_or_default();
            log_i!("[CDN-SCHEDULE] Следующее еженедельное сканирование CDN запланировано на {}", next_run.format("%Y-%m-%d %H:%M:%S"));

            let mut elapsed = std::time::Duration::ZERO;
            while elapsed < wait_dur {
                if failover::is_shutdown() {
                    return;
                }
                let step = std::cmp::min(std::time::Duration::from_secs(10), wait_dur - elapsed);
                tokio::time::sleep(step).await;
                elapsed += step;
            }
            if failover::is_shutdown() {
                return;
            }

            let cfg = weekly_state.config.read().await;
            if !cfg.force_domains.is_empty() {
                log_i!("[CDN-SCHEDULE] ⏰ Запуск планового еженедельного сканирования CDN (понедельник 05:00)...");
                let auto_cdns = cdn_discovery::discover_all_cdns(&cfg.force_domains, &cfg.mihomo_proxy_url()).await;
                let mut all_domains = cfg.force_domains.clone();
                for cdn in &auto_cdns {
                    if !all_domains.contains(cdn) {
                        all_domains.push(cdn.clone());
                    }
                }
                if let Err(e) = override_sync::sync_geo_override(&all_domains).await {
                    log_w!("[CDN-SCHEDULE] Ошибка плановой синхронизации geo_override: {}", e);
                } else {
                    log_i!("[CDN-SCHEDULE] ✓ Плановое еженедельное сканирование CDN успешно завершено");
                }
            }
        }
    });

    let auth_state = state.clone();

    let app = Router::new()
        // Авторизация
        .route("/api/auth/status", get(auth::auth_status))
        .route("/api/auth/login", post(auth::login))
        .route("/api/auth/logout", post(auth::logout))
        .route("/api/auth/password", post(auth::change_password))
        // Статус и метрики
        .route("/api/status", get(api::status))
        .route("/api/system/metrics", get(api::get_system_metrics))
        .route("/api/system/processes", get(api::get_system_processes))
        .route("/api/system/processes/kill", post(api::kill_process))
        // Серверы
        .route("/api/servers", get(api::get_servers))
        .route("/api/servers/switch", post(api::switch_server))
        .route("/api/servers/active", put(api::switch_server))
        .route("/api/servers/ping", post(api::ping_servers))
        .route("/api/servers/fix-names", post(api::fix_names))
        .route("/api/servers/google-check", get(api::check_google_geo))
        .route("/api/flow/status", get(api::get_flow_status))
        .route("/api/flow/switch", post(api::switch_flow_server))
        .route("/api/flow/ping", post(api::ping_flow_servers))
        .route("/api/flow/repair", post(api::repair_flow))
        .route("/api/flow/extension.zip", get(api::get_flow_extension))
        .route("/api/servers/speedtest", post(api::speedtest_server))
        .route("/api/servers/speedtest/{id}", post(api::speedtest_server_by_id).get(api::speedtest_server_by_id))
        .route("/api/servers/import-node", post(api::import_node))
        // Устройства и маршрутизация
        .route("/api/devices", get(api::get_devices))
        .route("/api/devices/traffic", get(api::get_devices_traffic))
        .route("/api/devices/policy", post(api::set_device_policy))
        .route("/api/devices/speed", post(api::set_device_speed))
        .route("/api/devices/domain-rules", get(api::get_device_domain_rules).post(api::set_device_domain_rules))
        .route("/api/policies", get(api::get_policies))
        .route("/api/routing", get(api::get_routing).post(api::apply_routing))
        .route("/api/device-routing", get(api::get_device_routing).post(api::set_device_routing))
        .route("/api/domains", get(api::get_domains).post(api::set_domains))
        .route("/api/domains/force-add", post(api::force_add_domain))
        .route("/api/domains/scan-cdn", post(api::scan_cdn_manual))
        .route("/api/dns/mode", get(api::get_dns_mode).post(api::set_dns_mode))
        // Сервис XKeen, бэкапы, конфиги
        .route("/api/xkeen/service", post(api::xkeen_service))
        .route("/api/config-files/list", get(api::list_config_files))
        .route("/api/config-files/read", get(api::read_config_file))
        .route("/api/config-files/save", post(api::save_config_file))
        .route("/api/backups", get(api::list_backups).post(api::create_backup))
        .route("/api/backups/restore", post(api::restore_backup))
        .route("/api/backups/delete", post(api::delete_backup))
        .route("/api/backups/export/{name}", get(api::export_backup))
        .route("/api/backups/import", post(api::import_backup))
        .route("/api/ignore", get(api::get_ignore).post(api::set_ignore))
        // Failover и уведомления
        .route("/api/failover/check", post(api::failover_check))
        .route("/api/failover/toggle", post(api::failover_toggle).put(api::failover_toggle))
        .route("/api/failover/events", get(api::failover_events))
        .route("/api/notifications/test", post(api::test_notification))
        // Провайдеры
        .route("/api/providers", get(api::get_providers))
        .route("/api/providers/rename", post(api::rename_provider))
        .route("/api/providers/update", post(api::update_provider))
        .route("/api/providers/add", post(api::add_provider))
        .route("/api/providers/delete", post(api::delete_provider))
        // Настройки и логи
        .route("/api/settings", get(api::get_settings).put(api::put_settings))
        .route("/api/settings/priority", post(api::set_priority))
        .route("/api/logs", get(api::logs_tail))
        .route("/api/logs/mihomo", get(api::mihomo_logs_tail))
        .route("/api/logs/mihomo/clear", post(api::mihomo_logs_clear))
        .route("/api/logs/download", get(api::logs_download))
        .route("/api/logs/clear", post(api::logs_clear))
        .route("/api/logs/ws", get(api::logs_ws))
        // Обновление и Antigravity
        .route("/api/update/check", get(crate::updater::check))
        .route("/api/update/install", post(crate::updater::install))
        .route("/api/mihomo/releases", get(crate::updater::mihomo_releases))
        .route("/api/mihomo/install", post(crate::updater::mihomo_install))
        .route("/api/antigravity/status", get(api::get_antigravity_status))
        .route("/api/antigravity/settings", post(api::set_antigravity_settings))
        .route("/api/antigravity/check", post(api::check_antigravity))
        .route("/api/antigravity/fix.cmd", get(api::get_antigravity_fix_cmd))
        .route("/patch", get(api::get_antigravity_patch_script))
        // Reverse-Proxy Clash API
        .route("/clash/{*path}", any(api::clash_proxy))
        // AdBlock (Блокировка рекламы)
        .route("/api/adblock", get(api::get_adblock))
        .route("/api/adblock/toggle", post(api::toggle_adblock))
        // GeoIP & GeoSite
        .route("/api/system/geo-info", get(api::get_geo_info))
        .route("/api/system/geo-update", post(api::update_geo_databases))
        // Connections Viewer
        .route("/api/connections", get(api::get_connections).delete(api::close_connections))
        .route("/api/connections/{id}", delete(api::close_single_connection))
        // Монитор трафика
        .route("/api/traffic/poll", get(api::get_traffic_poll))
        // Rules Viewer и «Куда пойдёт?»
        .route("/api/rules", get(api::get_rules))
        .route("/api/rules/test", post(api::test_rule_match))
        // Network Diagnostics & Smart DNS
        .route("/api/diagnostics/health", get(api::get_diagnostics_health))
        .route("/api/diagnostics/dns-test", post(api::test_dns_domain))
        .route("/api/dns/clean-servers", post(api::apply_clean_dns))
        // Keenetic Policies Map
        .route("/api/policies/map", get(api::get_policies_map))
        // Zapret / DPI
        .route("/api/zapret/status", get(api::get_zapret_status))
        .route("/api/zapret/action", post(api::zapret_action))
        // Расписания устройств
        .route("/api/schedules", get(api::get_schedules).post(api::save_schedules))
        // Игровой режим (Gaming Mode)
        .route("/api/gaming/status", get(api::get_gaming_status))
        .route("/api/gaming/save", post(api::save_gaming_config))
        .route("/api/gaming/toggle", post(api::toggle_gaming))
        .route("/api/gaming/ping", post(api::ping_gaming_targets))
        // 404 JSON для несуществующих маршрутов API (вместо отдачи HTML через SPA fallback)
        .route("/api/{*path}", any(api::api_not_found))
        .fallback(frontend::serve)
        .layer(middleware::from_fn_with_state(auth_state, auth::auth_middleware))
        .layer(middleware::from_fn(no_cache))
        .layer(middleware::from_fn(log_requests))
        .with_state(state.clone());

    let host_ip: std::net::IpAddr = cli.host.parse().unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::new(0, 0, 0, 0)));
    let addr = std::net::SocketAddr::new(host_ip, port);
    let listener = {
        let mut attempts = 0;
        loop {
            match tokio::net::TcpListener::bind(addr).await {
                Ok(l) => break l,
                Err(e) if attempts < 10 => {
                    log_w!(
                        "Порт {} временно недоступен ({}/10): {}. Повтор через 2 сек...",
                        port,
                        attempts + 1,
                        e
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    attempts += 1;
                }
                Err(e) => {
                    log_e!("Критическая ошибка: не удалось занять адрес {} после 10 попыток: {}", addr, e);
                    std::process::exit(1);
                }
            }
        }
    };
    log_i!("Панель доступна на http://{}:{}", host_ip, port);
    {
        let cfg = state.config.read().await;
        if !cfg.auth.enabled {
            log_w!("⚠️ ВНИМАНИЕ: Авторизация отключена! Панель слушает http://{}:{} без пароля. Рекомендуется настроить пароль в интерфейсе.", host_ip, port);
        }
    }
    if let Err(e) = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    {
        log_e!("Ошибка работы HTTP сервера: {}", e);
    }
    log_i!("Остановка: новые соединения закрыты, завершаю фоновые задачи…");
    antigravity::shutdown();
    failover::shutdown();
    watchdog::shutdown();
    traffic::shutdown();
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    log_i!("XKeen Route остановлен");
}

/// Ожидание SIGINT/SIGTERM (init-скрипт Entware шлёт SIGTERM).
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut term = signal(SignalKind::terminate()).expect("SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = term.recv() => {},
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
    log_i!("Получен сигнал остановки (graceful shutdown)");
}

pub fn chrono_ts() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Вычисление интервала ожидания до следующего понедельника 05:00:00 (по местному времени роутера)
pub fn duration_until_next_monday_5am() -> std::time::Duration {
    use chrono::{Datelike, Local, NaiveTime};
    let now = Local::now();
    let today = now.date_naive();
    let target_time = NaiveTime::from_hms_opt(5, 0, 0).unwrap_or_default();
    let weekday = now.weekday().num_days_from_monday(); // 0 = Mon, ..., 6 = Sun

    let target_date = if weekday == 0 && now.time() < target_time {
        today
    } else {
        let days_ahead = if weekday == 0 { 7 } else { 7 - weekday };
        today + chrono::Duration::days(days_ahead as i64)
    };

    let target_dt = target_date
        .and_time(target_time)
        .and_local_timezone(Local)
        .single()
        .unwrap_or_else(|| now + chrono::Duration::days(7));

    let diff = target_dt.signed_duration_since(now);
    if diff.num_seconds() > 0 {
        std::time::Duration::from_secs(diff.num_seconds() as u64)
    } else {
        std::time::Duration::from_secs(7 * 86400)
    }
}
