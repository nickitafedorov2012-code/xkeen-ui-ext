mod api;
mod config;
mod failover;
mod frontend;
mod logger;
mod mihomo;
mod rci;
mod routing;
pub mod transaction;
pub mod zapret;
mod updater;
mod override_sync;
mod cdn_discovery;
mod antigravity;
mod auth;
mod watchdog;
mod speedtest;
mod notifications;
pub mod traffic;
pub mod direct_traffic;
mod system;
pub mod adguard;
pub mod bot;
pub mod whitelist_catalog;
pub mod subscription_parser;

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
    version = crate::VERSION,
    about = "XKeen Route — веб-панель управления маршрутизацией Keenetic/Mihomo",
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
    /// Сериализация операций Zapret (start, stop, restart, fw, lockfile).
    pub zapret_lock: Arc<tokio::sync::Mutex<()>>,
    /// Менеджер обхода блокировок Google Antigravity.
    pub antigravity: Arc<antigravity::AntigravityManager>,
}

fn init_script_content(port: u16) -> String {
    format!(
        r#"#!/bin/sh
# XKeen Route: автономный init-скрипт Entware.
# S99 запускается после S05xkeen. Дополнительные проверки устраняют гонку,
# когда /opt или Mihomo ещё не готовы после полной перезагрузки роутера.

PATH=/opt/sbin:/opt/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
NAME=xkeen-route
BIN=/opt/sbin/xkeen-route
PID_FILE=/opt/var/run/xkeen-route.pid
LOG_FILE=/opt/etc/xkeen-route/xkeen-route.log
XKEEN_INIT=/opt/etc/init.d/S05xkeen
PORT={port}

is_running() {{
    if [ -s "$PID_FILE" ]; then
        pid="$(cat "$PID_FILE" 2>/dev/null)"
        case "$pid" in ''|*[!0-9]*) pid="" ;; esac
        if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
            if [ -r "/proc/$pid/cmdline" ] && tr '\000' ' ' < "/proc/$pid/cmdline" | grep -q "$NAME"; then
                return 0
            fi
        fi
    fi
    for p in $(pidof "$NAME" 2>/dev/null); do
        if kill -0 "$p" 2>/dev/null; then
            echo "$p" > "$PID_FILE"
            return 0
        fi
    done
    return 1
}}

wait_for_opt() {{
    attempt=0
    while [ "$attempt" -lt 120 ]; do
        [ -x "$BIN" ] && return 0
        attempt=$((attempt + 1))
        sleep 0.5
    done
    echo "$NAME: $BIN is unavailable after waiting for /opt" >&2
    return 1
}}

mihomo_running() {{
    pidof mihomo clash clash-meta >/dev/null 2>&1
}}

ensure_xkeen() {{
    attempt=0
    while [ "$attempt" -lt 12 ]; do
        mihomo_running && return 0
        if [ -x "$XKEEN_INIT" ]; then
            sh "$XKEEN_INIT" start >/dev/null 2>&1 || true
        fi
        attempt=$((attempt + 1))
        sleep 5
    done
    echo "$NAME: Mihomo did not start in 60 seconds; panel will remain available for diagnostics" >&2
    return 1
}}

start() {{
    wait_for_opt || return 1
    mkdir -p "$(dirname "$PID_FILE")" "$(dirname "$LOG_FILE")"

    # Панель стартует первой: при сбое Mihomo она остаётся доступна для диагностики.
    if ! is_running; then
        rm -f "$PID_FILE"
        "$BIN" -p "$PORT" >> "$LOG_FILE" 2>&1 &
        pid=$!
        sleep 1
        if kill -0 "$pid" 2>/dev/null; then
            echo "$pid" > "$PID_FILE"
        else
            echo "$NAME: failed to start; see $LOG_FILE" >&2
            return 1
        fi
    fi

    # Не задерживаем загрузку Entware: Mihomo восстанавливается ретраями в фоне.
    # Повторный start из cron выполняет ту же проверку.
    ensure_xkeen >> "$LOG_FILE" 2>&1 &
    return 0
}}

stop() {{
    if is_running; then
        pid="$(cat "$PID_FILE")"
        kill -TERM "$pid" 2>/dev/null || true
        attempt=0
        while kill -0 "$pid" 2>/dev/null && [ "$attempt" -lt 10 ]; do
            attempt=$((attempt + 1))
            sleep 1
        done
        kill -0 "$pid" 2>/dev/null && kill -KILL "$pid" 2>/dev/null || true
    fi
    rm -f "$PID_FILE"
}}

status() {{
    if is_running; then
        echo "$NAME alive (pid $(cat "$PID_FILE"))"
        return 0
    fi
    echo "$NAME stopped"
    return 1
}}

case "$1" in
    start) start ;;
    stop) stop ;;
    restart) stop; start ;;
    status) status ;;
    *) echo "Usage: $0 {{start|stop|restart|status}}" >&2; exit 1 ;;
esac
"#
    )
}

fn create_init(port: u16) -> std::io::Result<()> {
    if let Some(dir) = PathBuf::from(INIT_SCRIPT).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(INIT_SCRIPT, init_script_content(port))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(INIT_SCRIPT, std::fs::Permissions::from_mode(0o755))?;
        // Резервная проверка: start идемпотентен и одновременно восстанавливает Mihomo.
        let _ = std::process::Command::new("sh")
            .arg("-c")
            .arg("(crontab -l 2>/dev/null | grep -v 'xkeen-route'; echo '*/5 * * * * /opt/etc/init.d/S99xkeen-route start >/dev/null 2>&1') | crontab -")
            .status();
    }
    println!("[OK] Init-скрипт создан: {}", INIT_SCRIPT);
    Ok(())
}

#[cfg(test)]
mod init_script_tests {
    use super::init_script_content;

    #[test]
    fn init_script_waits_for_entware_and_recovers_mihomo() {
        let script = init_script_content(1001);
        assert!(script.contains("wait_for_opt()"));
        assert!(script.contains("while [ \"$attempt\" -lt 120 ]; do"));
        assert!(script.contains("sleep 0.5"));
        assert!(script.contains("ensure_xkeen()"));
        assert!(script.contains("sh \"$XKEEN_INIT\" start"));
        assert!(script.contains("Mihomo did not start in 60 seconds"));
    }

    #[test]
    fn init_script_uses_pid_file_and_requested_port() {
        let script = init_script_content(1234);
        assert!(script.contains("PID_FILE=/opt/var/run/xkeen-route.pid"));
        assert!(script.contains("PORT=1234"));
        assert!(script.contains("$BIN\" -p \"$PORT\""));
        assert!(script.contains("for p in $(pidof \"$NAME\" 2>/dev/null); do"));
    }

    #[tokio::test]
    async fn test_startup_whitelist_gaming_auto_recovery_no_deadlock() {
        let mut cfg = crate::config::AppConfig::default();
        cfg.gaming.enabled = false;
        cfg.whitelist.gaming_suspended = true;

        let config_state = std::sync::Arc::new(tokio::sync::RwLock::new(std::sync::Arc::new(cfg)));
        let config_lock = std::sync::Arc::new(tokio::sync::Mutex::new(()));

        // 1. Scoped read (must not deadlock write)
        let (gaming_suspended, is_bs) = {
            let read_guard = config_state.read().await;
            (read_guard.whitelist.gaming_suspended, false)
        };

        // 2. Write lock acquisition
        if gaming_suspended && !is_bs {
            let _cfg_guard = config_lock.lock().await;
            let mut current_cfg = (**config_state.read().await).clone();
            current_cfg.gaming.enabled = true;
            current_cfg.whitelist.gaming_suspended = false;
            *config_state.write().await = std::sync::Arc::new(current_cfg);
        }

        let final_cfg = config_state.read().await;
        assert!(final_cfg.gaming.enabled);
        assert!(!final_cfg.whitelist.gaming_suspended);
    }
}


/// Управление заголовками кэширования ответов веб-сервера:
/// - Хэшированные ассеты фронтенда (/assets/*) при успехе кэшируются на 1 год как неизменяемые (immutable).
/// - index.html, API (/api/*) и динамические ответы получают no-cache, no-store, must-revalidate.
async fn no_cache(req: Request, next: Next) -> Response {
    let path = req.uri().path();
    let is_asset = path.starts_with("/assets/");
    let mut res = next.run(req).await;

    if is_asset && res.status().is_success() {
        if let Ok(v) = axum::http::HeaderValue::from_str("public, max-age=31536000, immutable") {
            res.headers_mut().insert(axum::http::header::CACHE_CONTROL, v);
        }
    } else if !res.headers().contains_key(axum::http::header::CACHE_CONTROL) {
        if let Ok(v) = axum::http::HeaderValue::from_str("no-cache, no-store, must-revalidate") {
            res.headers_mut().insert(axum::http::header::CACHE_CONTROL, v);
        }
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
        zapret_lock: Arc::new(tokio::sync::Mutex::new(())),
        antigravity: ag_mgr,
    };

    failover::spawn(state.clone());
    watchdog::spawn(state.clone());
    traffic::spawn(state.clone());
    bot::spawn(state.clone());

    // Синхронизация файлов и Keenetic ndm хуков Zapret при старте панели
    let zapret_sync_state = state.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(6)).await;
        let cfg = zapret_sync_state.config.read().await;
        if cfg.zapret.enabled {
            // Z-03: служба включена в конфигурации — маркер автозапуска должен отсутствовать
            zapret::clear_zapret_disabled_marker();
            if let Err(e) = api::sync_zapret_files(&cfg.zapret).await {
                log_w!("[STARTUP] Предупреждение: не удалось синхронизировать файлы Zapret: {}", e);
            }
        }
    });

    // Синхронизация режима игровой совместимости (iptables full tunnel) при старте панели
    let gaming_sync_state = state.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(7)).await;
        let cfg = gaming_sync_state.config.read().await;
        if cfg.gaming.enabled && cfg.gaming.mode == config::GamingMode::Compatibility {
            if let Ok(yaml) = tokio::fs::read_to_string(&cfg.mihomo.config_path).await {
                let redir_port = routing::parse_redir_port(&yaml).unwrap_or(5000);
                let tproxy_port = routing::parse_tproxy_port(&yaml).unwrap_or(5001);
                let dns_port = api::resolve_active_dns_port(&cfg);
                let _ = api::sync_gaming_full_tunnel(&cfg.gaming, redir_port, tproxy_port, dns_port).await;
            }
        }
    });

    // PING-AUTO: фоновое автообновление пингов серверов (каждые 30 секунд замеряет
    // ограниченный набор: цепочка failover + основной узел + Fastest/Fallback/PROXY).
    // Замер через mihomo delay-тест обновляет историю нод, поэтому GET /api/servers
    // и 3-секундный опрос UI отдают живые пинги без действий пользователя в панели.
    let ping_auto_state = state.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(15)).await;
        loop {
            let cfg = ping_auto_state.config.read().await.clone();
            let ids = crate::mihomo::collect_auto_ping_ids(&cfg);
            if !ids.is_empty() {
                let _ = crate::mihomo::ping_all_url(&ping_auto_state.http, &cfg, &ids, 2500, None).await;
            }
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        }
    });

    // Синхронизация AdGuard Home DNS-редиректа при старте панели
    let adguard_sync_state = state.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(4)).await;
        let cfg = adguard_sync_state.config.read().await.clone();
        let agh_status = adguard::get_status(&adguard_sync_state.http, &cfg.adguard).await;
        let is_running_protected = agh_status.running && agh_status.protection_enabled;
        let is_enabled = cfg.adguard.enabled || cfg.adblock_enabled || is_running_protected;
        if agh_status.running {
            let _ = adguard::heal_adguard_live_config(&adguard_sync_state.http, &cfg.adguard, Some(&cfg.rci.host)).await;
        }
        if is_enabled {
            let active_port = adguard::detect_adguard_dns_port().unwrap_or(if agh_status.dns_port > 0 { agh_status.dns_port } else { cfg.adguard.dns_port });
            let listener_ok = adguard::test_agh_dns_listener(&cfg.adguard.host, active_port).await;
            if listener_ok {
                let _ = adguard::apply_adguard_iptables(active_port);
                log_i!("[STARTUP] ✓ AdGuard Home DNS redirection to port {} applied", active_port);
                if cfg.gaming.enabled && cfg.gaming.mode == config::GamingMode::Compatibility {
                    // Z-09: порты парсим из config.yaml, а не хардкодим 5000/5001 —
                    // при кастомном конфиге Mihomo правила уходили не на те порты
                    if let Ok(yaml) = tokio::fs::read_to_string(&cfg.mihomo.config_path).await {
                        let redir_port = routing::parse_redir_port(&yaml).unwrap_or(5000);
                        let tproxy_port = routing::parse_tproxy_port(&yaml).unwrap_or(5001);
                        let _ = api::sync_gaming_full_tunnel(&cfg.gaming, redir_port, tproxy_port, active_port).await;
                    }
                }
            }

            // Автоматическая синхронизация AdGuard DNS upstream в config.yaml и config.json
            if let Ok(mut tx) = crate::transaction::ConfigTx::begin(&adguard_sync_state).await {
                let mut needs_save = false;
                if is_running_protected && (!tx.config().adguard.enabled || tx.config().adguard.dns_port != active_port) {
                    tx.config_mut().adguard.enabled = true;
                    tx.config_mut().adguard.dns_port = active_port;
                    needs_save = true;
                }
                if let Ok(raw_yaml) = tx.read_yaml().await {
                    if !raw_yaml.contains("AUTO-ADGUARD-DNS-BEGIN") {
                        if let Ok((new_yaml, _)) = crate::routing::apply_routing(&raw_yaml, tx.config()) {
                            let _ = tx.set_yaml(new_yaml);
                            needs_save = true;
                        }
                    }
                }
                if needs_save {
                    let _ = tx.commit_and_reload().await;
                    log_i!("[STARTUP] ✓ AdGuard Home DNS upstream synced into Mihomo config");
                }
            }
        }
    });

    // Синхронизация режима обхода белых списков (100% full tunnel) при старте панели
    let wl_sync_state = state.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(8)).await;
        let (active_id, _active_srv, is_bs, gaming_suspended) = {
            let cfg = wl_sync_state.config.read().await;
            let active_srv = mihomo::get_active_server(&wl_sync_state.http, &cfg).await.ok().flatten();
            let active_id = active_srv.as_ref().map(|s| s.id.clone()).unwrap_or_default();
            let is_bs = active_srv.as_ref().map(|s| failover::is_whitelist_server(&s.name, &s.id, &cfg)).unwrap_or(false);
            let gaming_suspended = cfg.whitelist.gaming_suspended;
            (active_id, active_srv, is_bs, gaming_suspended)
        };
        if !active_id.is_empty() {
            let _ = api::sync_whitelist_mode_tunnel(&wl_sync_state, &active_id).await;
        }

        // Авто-восстановление игрового режима при перезагрузке роутера:
        // Если флаг gaming_suspended == true, а текущий активный сервер не является [БС]
        if gaming_suspended && !is_bs {
            let _cfg_guard = wl_sync_state.config_lock.lock().await;
            let mut current_cfg = (**wl_sync_state.config.read().await).clone();
            current_cfg.gaming.enabled = true;
            current_cfg.whitelist.gaming_suspended = false;
            if let Ok(_) = config::save(&wl_sync_state.config_path, &current_cfg).await {
                *wl_sync_state.config.write().await = std::sync::Arc::new(current_cfg.clone());
                log_i!("[STARTUP] ✓ Игровой режим автоматически восстановлен после перезагрузки (активный узел не является [БС])");
                // X-03: восстановление не только флага, но и реальных правил —
                // стартовый 7-секундный синк уже прошёл с enabled=false, и без
                // этого блока UI показывает «включено», а трафик не туннелируется.
                if let Ok(raw_yaml) = tokio::fs::read_to_string(&current_cfg.mihomo.config_path).await {
                    if let Ok((new_yaml, _)) = crate::routing::apply_routing(&raw_yaml, &current_cfg) {
                        let _ = crate::api::atomic_write_file(&current_cfg.mihomo.config_path, &new_yaml).await;
                        let _ = mihomo::reload_config(&wl_sync_state.http, &current_cfg).await;
                        let redir_port = crate::routing::parse_redir_port(&new_yaml).unwrap_or(5000);
                        let tproxy_port = crate::routing::parse_tproxy_port(&new_yaml).unwrap_or(5001);
                        let dns_port = crate::api::resolve_active_dns_port(&current_cfg);
                        let _ = crate::api::sync_gaming_full_tunnel(&current_cfg.gaming, redir_port, tproxy_port, dns_port).await;
                        log_i!("[STARTUP] ✓ Правила игрового туннеля перемонтированы после восстановления");
                    }
                }
            }
        }
    });

    // Авто-миграция и плановое авто-обновление мобильных подписок (чистые имена серверов)
    let mobile_sub_state = state.clone();
    tokio::spawn(async move {
        // Ожидание старта Mihomo
        tokio::time::sleep(std::time::Duration::from_secs(12)).await;
        log_i!("[STARTUP] Запуск начальной проверки и синхронизации мобильных подписок...");
        let migrated = api::sync_all_mobile_providers(&mobile_sub_state).await;
        if migrated > 0 {
            log_i!("[STARTUP] ✓ Синхронизировано мобильных подписок: {migrated}");
        }

        // Периодическое обновление каждые 12 часов
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(43200)).await;
            if failover::is_shutdown() {
                break;
            }
            log_i!("[MOBILE-SUB] Плановое обновление мобильных подписок (12h)...");
            let _ = api::sync_all_mobile_providers(&mobile_sub_state).await;
        }
    });

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
        .route("/api/auth/change-password", post(auth::change_password))
        // Статус и метрики
        .route("/api/status", get(api::status))
        .route("/api/system/diagnostics", get(api::get_system_diagnostics))
        .route("/api/system/logs/recent", get(api::get_recent_logs))
        .route("/api/system/metrics", get(api::get_system_metrics))
        .route("/api/system/processes", get(api::get_system_processes))
        .route("/api/system/processes/kill", post(api::kill_process))
        .route("/api/system/stack-memory", get(api::get_stack_memory))
        .route("/api/system/swap/status", get(api::get_swap_status))
        .route("/api/system/swap/create", post(api::create_swap))
        .route("/api/system/swap/remove", post(api::remove_swap))
        .route("/api/system/swap", delete(api::remove_swap))
        // Серверы
        .route("/api/servers", get(api::get_servers))
        .route("/api/servers/alias", post(api::set_server_alias))
        .route("/api/servers/whitelist", post(api::set_server_whitelist))
        .route("/api/whitelist/providers", post(api::set_provider_whitelist))
        .route("/api/providers/whitelist", post(api::set_provider_whitelist))
        // Режим белых списков (Whitelist Mode)
        .route("/api/whitelist", get(api::get_whitelist).post(api::save_whitelist))
        .route("/api/whitelist/status", get(api::get_whitelist))
        .route("/api/whitelist/save", post(api::save_whitelist))
        .route("/api/whitelist/strategy", post(api::set_whitelist_strategy))
        .route("/api/whitelist/exclude", post(api::toggle_whitelist_exclude))
        .route("/api/whitelist/device/toggle", post(api::toggle_whitelist_device))
        .route("/api/whitelist/toggle", post(api::toggle_whitelist_device))
        .route("/api/whitelist/catalog", get(api::get_whitelist_catalog))
        .route("/api/servers/switch", post(api::switch_server))
        .route("/api/servers/active", put(api::switch_server))
        .route("/api/servers/ping", post(api::ping_servers))
        .route("/api/servers/fix-names", post(api::fix_names))
        .route("/api/servers/google-check", get(api::check_google_geo))
        .route("/api/flow/status", get(api::get_flow_status))
        .route("/api/flow/switch", post(api::switch_flow_server))
        .route("/api/flow/auto-select", post(api::auto_select_flow_server))
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
        .route("/api/devices/zapret", get(api::get_devices_zapret))
        .route("/api/devices/zapret-toggle", post(api::toggle_device_zapret))
        .route("/api/policies", get(api::get_policies))
        .route("/api/routing", get(api::get_routing).post(api::apply_routing))
        .route("/api/device-routing", get(api::get_device_routing).post(api::set_device_routing))
        .route("/api/domains", get(api::get_domains).post(api::set_domains))
        .route("/api/domains/force-add", post(api::force_add_domain))
        .route("/api/domains/scan-cdn", post(api::scan_cdn_manual))
        .route("/api/dns/mode", get(api::get_dns_mode).post(api::set_dns_mode))
        // Сервис XKeen, бэкапы, конфиги
        .route("/api/xkeen/service", post(api::xkeen_service))
        .route("/api/config/apply", post(api::apply_config_tx))
        .route("/api/config/confirm", post(api::confirm_config_tx))
        .route("/api/config/rollback", post(api::rollback_config_tx))
        .route("/api/config/validate", post(api::validate_config))
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
        .route("/api/failover/quarantine", get(api::failover_quarantine_list))
        .route("/api/failover/quarantine/clear", post(api::failover_quarantine_clear))
        .route("/api/notifications/test", post(api::test_notification))
        // Провайдеры
        .route("/api/providers", get(api::get_providers))
        .route("/api/providers/toggle", post(api::toggle_provider))
        .route("/api/providers/rename", post(api::rename_provider))
        .route("/api/providers/update", post(api::update_provider))
        .route("/api/providers/hwid", post(api::update_provider_hwid))
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
        // Xbox DNS & SmartDNS
        .route("/api/xbox-dns/status", get(api::get_xbox_dns_status))
        .route("/api/xbox-dns/check", post(api::check_xbox_dns))
        .route("/api/xbox-dns/fix.cmd", get(api::get_xbox_dns_fix_cmd))
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
        .route("/api/traffic/reset", post(api::post_traffic_reset))
        // Rules Viewer и «Куда пойдёт?»
        .route("/api/rules", get(api::get_rules))
        .route("/api/rules/test", post(api::test_rule_match))
        // Network Diagnostics & Smart DNS
        .route("/api/diagnostics/health", get(api::get_diagnostics_health))
        .route("/api/diagnostics/dns-test", post(api::test_dns_domain))
        .route("/api/diagnostics/check-domain", get(api::check_domain))
        .route("/api/diagnostics/unblock-domain", post(api::unblock_domain))
        .route("/api/dns/clean-servers", post(api::apply_clean_dns))
        // Keenetic Policies Map
        .route("/api/policies/map", get(api::get_policies_map))
        // Zapret / DPI
        .route("/api/zapret/status", get(api::get_zapret_status))
        .route("/api/zapret/action", post(api::zapret_action))
        .route("/api/zapret/analytics", get(api::get_zapret_analytics))
        .route("/api/zapret/blockcheck", post(api::run_mini_blockcheck))
        .route("/api/zapret/community-hostlist/sync", post(api::sync_community_hostlist))
        .route("/api/zapret/update/check", get(api::check_zapret_update_route))
        // Расписания устройств
        .route("/api/schedules", get(api::get_schedules).post(api::save_schedules))
        // Игровой режим (Gaming Mode)
        .route("/api/gaming/status", get(api::get_gaming_status))
        .route("/api/gaming/save", post(api::save_gaming_config))
        .route("/api/gaming/toggle", post(api::toggle_gaming))
        .route("/api/gaming/ping", post(api::ping_gaming_targets))
        .route("/api/gaming/ignore-conn", post(api::ignore_gaming_conn))
        // AdGuard Home
        .route("/api/adguard/status", get(api::adguard_status))
        .route("/api/adguard/capabilities", get(api::adguard_capabilities))
        .route("/api/adguard/health", get(api::adguard_health))
        .route("/api/adguard/overview", get(api::adguard_overview))
        .route("/api/adguard/query-log", get(api::adguard_query_log))
        .route("/api/adguard/protection", post(api::adguard_set_protection))
        .route("/api/adguard/filtering", get(api::adguard_get_filtering))
        .route("/api/adguard/filtering/rules", post(api::adguard_set_rules))
        .route("/api/adguard/filtering/toggle-filter", post(api::adguard_toggle_filter))
        .route("/api/adguard/rewrites", get(api::adguard_get_rewrites))
        .route("/api/adguard/rewrites/add", post(api::adguard_add_rewrite))
        .route("/api/adguard/rewrites/delete", post(api::adguard_delete_rewrite))
        .route("/api/adguard/clients", get(api::adguard_get_clients))
        .route("/api/adguard/diagnostics", get(api::adguard_diagnostics))
        .route("/api/adguard/service", post(api::adguard_service))
        .route("/api/adguard/install", post(api::adguard_install))
        .route("/api/adguard/config", get(api::adguard_get_config).post(api::adguard_set_config))
        // 404 JSON для несуществующих маршрутов API (вместо отдачи HTML через SPA fallback)
        .route("/api/{*path}", any(api::api_not_found))
        .fallback(frontend::serve)
        .layer(axum::extract::DefaultBodyLimit::max(10 * 1024 * 1024))
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
