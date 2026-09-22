mod api;
mod config;
mod failover;
mod frontend;
mod logger;
mod mihomo;
mod rci;
mod routing;
mod updater;
mod override_sync;
mod cdn_discovery;
mod antigravity;

use axum::extract::Request;
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{get, post};
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

const INIT_SCRIPT_CONTENT: &str = r#"#!/bin/sh

ENABLED=yes
PROCS=xkeen-route
ARGS="-p 1001"
PREARGS=""
DESC=$PROCS
PATH=/opt/sbin:/opt/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

. /opt/etc/init.d/rc.func
"#;

#[derive(Parser)]
#[command(
    name = "xkeen-route",
    about = "XKeen Route — веб-панель управления маршрутизацией Keenetic/Mihomo",
    disable_version_flag = true,
    disable_help_subcommand = true
)]
struct Cli {
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
    /// Менеджер обхода блокировок Google Antigravity.
    pub antigravity: Arc<antigravity::AntigravityManager>,
}



fn create_init() -> std::io::Result<()> {
    if let Some(dir) = PathBuf::from(INIT_SCRIPT).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(INIT_SCRIPT, INIT_SCRIPT_CONTENT)?;
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
    if path != "/api/status" {
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

    match cli.command {
        Some(Command::Version) => {
            println!("{} {}", APP_NAME, VERSION);
            return;
        }
        Some(Command::CreateInit) => {
            if let Err(e) = create_init() {
                eprintln!("[ERROR] {}", e);
                std::process::exit(1);
            }
            return;
        }
        None => {}
    }

    let config_path = PathBuf::from(cli.config.clone().unwrap_or_else(|| CONFIG_PATH.to_string()));
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
        http: reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .cookie_store(true)
            .build()
            .expect("http client"),
        failover_log: Arc::new(failover::FailoverLog::default()),
        routing_lock: Arc::new(tokio::sync::Mutex::new(())),
        config_lock: Arc::new(tokio::sync::Mutex::new(())),
        antigravity: ag_mgr,
    };

    failover::spawn(state.clone());

    // Начальная и периодическая синхронизация IP принудительно проксируемых доменов и их CDN с geo_override
    let sync_state = state.clone();
    tokio::spawn(async move {
        // Задержка и ожидание готовности роутера/DNS после перезагрузки
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        wait_for_router_ready(&sync_state).await;

        let cfg = sync_state.config.read().await;
        if !cfg.force_domains.is_empty() {
            log_i!("[STARTUP] Начало обнаружения CDN и синхронизации geo_override...");
            let auto_cdns = cdn_discovery::discover_all_cdns(&cfg.force_domains).await;
            let mut all_domains = cfg.force_domains.clone();
            all_domains.extend(auto_cdns);
            if let Err(e) = override_sync::sync_geo_override(&all_domains).await {
                log_w!("[STARTUP] Ошибка синхронизации geo_override: {}", e);
            }
        }
    });

    let periodic_state = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1800));
        interval.tick().await; // пропускаем немедленный первый тик, т.к. начальная синхронизация уже запущена
        loop {
            interval.tick().await;
            let cfg = periodic_state.config.read().await;
            if !cfg.force_domains.is_empty() {
                let auto_cdns = cdn_discovery::discover_all_cdns(&cfg.force_domains).await;
                let mut all_domains = cfg.force_domains.clone();
                all_domains.extend(auto_cdns);
                if let Err(e) = override_sync::sync_geo_override(&all_domains).await {
                    log_w!("[PERIODIC] Ошибка периодической синхронизации geo_override: {}", e);
                }
            }
        }
    });


    let app = Router::new()
        .route("/api/status", get(api::status))
        .route("/api/system/metrics", get(api::get_system_metrics))
        .route("/api/servers", get(api::get_servers))
        .route("/api/servers/switch", post(api::switch_server))
        .route("/api/servers/ping", post(api::ping_servers))
        .route("/api/servers/fix-names", post(api::fix_names))
        .route("/api/devices", get(api::get_devices))
        .route("/api/devices/policy", post(api::set_device_policy))
        .route("/api/devices/speed", post(api::set_device_speed))
        .route("/api/policies", get(api::get_policies))
        .route("/api/routing", get(api::get_routing).post(api::apply_routing))
        .route("/api/device-routing", get(api::get_device_routing).post(api::set_device_routing))
        .route("/api/domains", get(api::get_domains).post(api::set_domains))
        .route("/api/xkeen/service", post(api::xkeen_service))
        .route("/api/backups", get(api::list_backups).post(api::create_backup))
        .route("/api/backups/restore", post(api::restore_backup))
        .route("/api/backups/delete", post(api::delete_backup))
        .route("/api/ignore", get(api::get_ignore).post(api::set_ignore))
        .route("/api/failover/check", post(api::failover_check))
        .route("/api/failover/toggle", post(api::failover_toggle))
        .route("/api/failover/events", get(api::failover_events))
        .route("/api/providers", get(api::get_providers))
        .route("/api/providers/rename", post(api::rename_provider))
        .route("/api/providers/update", post(api::update_provider))
        .route("/api/providers/add", post(api::add_provider))
        .route("/api/providers/delete", post(api::delete_provider))
        .route("/api/settings", get(api::get_settings).put(api::put_settings))
        .route("/api/settings/priority", post(api::set_priority))
        .route("/api/logs", get(api::logs_tail))
        .route("/api/logs/download", get(api::logs_download))
        .route("/api/logs/clear", post(api::logs_clear))
        .route("/api/logs/ws", get(api::logs_ws))
        .route("/api/update/check", get(crate::updater::check))
        .route("/api/update/install", post(crate::updater::install))
        .route("/api/antigravity/status", get(api::get_antigravity_status))
        .route("/api/antigravity/settings", post(api::set_antigravity_settings))
        .route("/api/antigravity/check", post(api::check_antigravity))
        .fallback(frontend::serve)
        .layer(middleware::from_fn(no_cache))
        .layer(middleware::from_fn(log_requests))
        .with_state(state);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
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
                    log_e!("Критическая ошибка: не удалось занять порт {} после 10 попыток: {}", port, e);
                    panic!("Не удалось занять порт {}: {}", port, e);
                }
            }
        }
    };
    log_i!("Панель доступна на http://0.0.0.0:{}", port);
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
    failover::shutdown();
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
