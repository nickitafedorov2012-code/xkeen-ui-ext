mod api;
mod config;
mod failover;
mod frontend;
mod mihomo;
mod rci;
mod routing;

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
    pub config: Arc<RwLock<config::AppConfig>>,
    pub config_path: Arc<PathBuf>,
    pub http: reqwest::Client,
    /// Разрешённый RCI-токен (из конфига или /opt/etc/xkeen/xkeen.json). Пустая строка = cookie-сессия.
    pub rci_token: Arc<RwLock<String>>,
    /// Лента событий failover.
    pub failover_log: Arc<failover::FailoverLog>,
    /// Сериализация правок config.yaml (гонки failover/ручных правок).
    pub routing_lock: Arc<tokio::sync::Mutex<()>>,
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
    println!("{} [INFO] {} запущен, конфиг: {}", chrono_ts(), APP_NAME, config_path.display());

    let port = cli.port;
    let state = AppState {
        config: Arc::new(RwLock::new(cfg)),
        config_path: Arc::new(config_path),
        http: reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .cookie_store(true)
            .build()
            .expect("http client"),
        rci_token: Arc::new(RwLock::new(String::new())),
        failover_log: Arc::new(failover::FailoverLog::default()),
        routing_lock: Arc::new(tokio::sync::Mutex::new(())),
    };

    failover::spawn(state.clone());


    let app = Router::new()
        .route("/api/status", get(api::status))
        .route("/api/servers", get(api::get_servers))
        .route("/api/servers/switch", post(api::switch_server))
        .route("/api/servers/ping", post(api::ping_servers))
        .route("/api/devices", get(api::get_devices))
        .route("/api/devices/policy", post(api::set_device_policy))
        .route("/api/devices/speed", post(api::set_device_speed))
        .route("/api/policies", get(api::get_policies))
        .route("/api/routing", get(api::get_routing).post(api::apply_routing))
        .route("/api/failover/check", post(api::failover_check))
        .route("/api/failover/events", get(api::failover_events))
        .route("/api/settings", get(api::get_settings).put(api::put_settings))
        .route("/api/settings/priority", post(api::set_priority))
        .fallback(frontend::serve)
        .layer(middleware::from_fn(no_cache))
        .with_state(state);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|e| panic!("Не удалось занять порт {}: {}", port, e));
    println!("{} [INFO] Панель доступна на http://0.0.0.0:{}", chrono_ts(), port);
    axum::serve(listener, app).await.unwrap();
}

pub fn chrono_ts() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}
