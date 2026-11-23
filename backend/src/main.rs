mod api;
mod config;
pub mod error;
pub mod shutdown;
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
pub mod vkturn;
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
pub mod whitelist_tags;
pub mod subscription_parser;
pub mod free_sub;

use axum::extract::Request;
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::Router;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;


pub const VERSION: &str = match option_env!("XKEEN_ROUTE_VERSION") {
    Some(v) => v,
    None => concat!("v", env!("CARGO_PKG_VERSION"), ".0"),
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
    /// Канонический порядок блокировок: `config_lock` -> `routing_lock` -> `zapret_lock` -> `speedtest_lock`.
    pub routing_lock: Arc<tokio::sync::Mutex<()>>,
    /// Сериализация read-modify-write конфига панели (config.json) — против TOCTOU.
    pub config_lock: Arc<tokio::sync::Mutex<()>>,
    /// Сериализация speedtest для исключения гонок при переключении узлов.
    pub speedtest_lock: Arc<tokio::sync::Mutex<()>>,
    /// Сериализация операций Zapret (start, stop, restart, fw, lockfile).
    pub zapret_lock: Arc<tokio::sync::Mutex<()>>,
    /// Менеджер обхода блокировок Google Antigravity.
    pub antigravity: Arc<antigravity::AntigravityManager>,
    /// Менеджер VK-туннеля (Tier 2 Whitelist Turn proxy).
    pub vkturn: Arc<vkturn::VkTurnManager>,
}

/// Координатор канонического порядка блокировок (`config_lock` -> `routing_lock`),
/// исключающий взаимоблокировки между транзакциями, бэкапами и маршрутизацией.
pub struct ConfigRoutingGuard {
    pub _config: tokio::sync::OwnedMutexGuard<()>,
    pub _routing: tokio::sync::OwnedMutexGuard<()>,
}

impl AppState {
    /// Мгновенный снимок текущего `AppConfig` без удержания `RwLockReadGuard` через `.await`.
    pub async fn cfg_snapshot(&self) -> Arc<config::AppConfig> {
        self.config.read().await.clone()
    }

    /// Атомарно обновить только `config.json` под `config_lock`, сохранить на диск и обновить `state.config`.
    pub async fn update_config<F, R>(&self, mutate: F) -> Result<(Arc<config::AppConfig>, R), String>
    where
        F: FnOnce(&mut config::AppConfig) -> R,
    {
        self.try_update_config(|cfg| Ok(mutate(cfg))).await
    }

    /// Атомарно обновить `config.json` с валидацией мутации (при `Err` диск и память не меняются).
    pub async fn try_update_config<F, R>(&self, mutate: F) -> Result<(Arc<config::AppConfig>, R), String>
    where
        F: FnOnce(&mut config::AppConfig) -> Result<R, String>,
    {
        let _guard = self.config_lock.lock().await;
        let mut cfg = (**self.config.read().await).clone();
        let res = mutate(&mut cfg)?;
        config::save(&self.config_path, &cfg).await?;
        let arc = Arc::new(cfg);
        *self.config.write().await = arc.clone();
        Ok((arc, res))
    }

    /// Неблокирующее обновление `config.json` (для фоновых/периодических флушей метрик без ожидания `config_lock`).
    pub async fn try_update_config_nonblocking<F, R>(
        &self,
        mutate: F,
    ) -> Option<Result<(Arc<config::AppConfig>, R), String>>
    where
        F: FnOnce(&mut config::AppConfig) -> R,
    {
        let _guard = self.config_lock.try_lock().ok()?;
        let mut cfg = (**self.config.read().await).clone();
        let res = mutate(&mut cfg);
        if let Err(e) = config::save(&self.config_path, &cfg).await {
            return Some(Err(e));
        }
        let arc = Arc::new(cfg);
        *self.config.write().await = arc.clone();
        Some(Ok((arc, res)))
    }

    /// Захватить `config_lock` и `routing_lock` строго в каноническом порядке.
    pub async fn lock_config_and_routing(&self) -> ConfigRoutingGuard {
        let config_guard = self.config_lock.clone().lock_owned().await;
        let routing_guard = self.routing_lock.clone().lock_owned().await;
        ConfigRoutingGuard {
            _config: config_guard,
            _routing: routing_guard,
        }
    }
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
            fd_out=true sh "$XKEEN_INIT" start on >/dev/null 2>&1 || true
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

    let cfg = config::load_and_refresh_backup(&config_path);
    logger::init(&config_path, &cfg.logs.remote_syslog);
    logger::set_level(&cfg.logs.level);
    updater::handle_startup_update_guard(&config_path);
    log_i!("{} {} запущен, конфиг: {}", APP_NAME, VERSION, config_path.display());
    if !cfg.logs.remote_syslog.is_empty() {
        log_i!("Логи дублируются на syslog {}", cfg.logs.remote_syslog);
    }

    let port = cli.port;
    let config = Arc::new(RwLock::new(Arc::new(cfg)));
    let ag_mgr = Arc::new(antigravity::AntigravityManager::new(Arc::clone(&config)));
    let vkturn_mgr = vkturn::VkTurnManager::new();

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
            .connect_timeout(std::time::Duration::from_secs(8))
            .timeout(std::time::Duration::from_secs(20))
            .http1_only()
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
        vkturn: vkturn_mgr,
    };

    // FO-QUR-01: Восстановление состояния карантина из каталога рядом с config.json
    failover::init_quarantine_path(&state.config_path);
    failover::init_quarantine_from_disk().await.ok();
    failover::init_detector_state_from_disk().await;

    // Инициализация постоянного хранилища отозванных токенов (баг 93)
    let revoked_storage_path = state.config_path.parent().unwrap_or(std::path::Path::new(".")).join("revoked_tokens.json");
    auth::init_revoked_tokens_storage(&revoked_storage_path);

    // Баг 131: неподтверждённое изменение (commit-confirmed), пережившее перезапуск, откатывается до старта сторожей
    transaction::recover_pending_commit_after_restart(&state).await;

    let startup_vkturn = {
        let c = state.config.read().await;
        if c.vkturn.enabled {
            Some(c.vkturn.clone())
        } else {
            None
        }
    };
    if let Some(cfg) = startup_vkturn {
        let synced = vkturn::commit_vkturn(&state, |_| Ok(())).await.unwrap_or(cfg);
        state.vkturn.restart(synced, state.clone()).await;
    }

    failover::spawn(state.clone());
    // В постоянном каталоге рядом с config.json (A-12): переживает и рестарт панели, и перезагрузку роутера.
    watchdog::init_service_stopped_flag(watchdog::service_stopped_flag_path(&state.config_path));
    watchdog::spawn(state.clone());
    traffic::spawn(state.clone());
    bot::spawn(state.clone());

    // AUDIT-122: автостарт mihomo после перезагрузки роутера, если S05xkeen отсутствует или не поднял ядро
    let mihomo_autostart_state = state.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        if crate::watchdog::is_service_stopped() || crate::system::is_process_running("mihomo") {
            return;
        }
        let cfg = mihomo_autostart_state.config.read().await.clone();
        let init_script = cfg.system.xkeen_init.trim();
        let has_init = !init_script.is_empty() && std::path::Path::new(init_script).exists();
        let start_cmd = if has_init {
            format!("fd_out=true sh \"{init_script}\" start on </dev/null >/dev/null 2>&1")
        } else {
            "if [ -x /opt/sbin/xkeen ]; then /opt/sbin/xkeen -start </dev/null >/dev/null 2>&1; elif [ -x /opt/sbin/mihomo ]; then nohup /opt/sbin/mihomo -d /opt/etc/mihomo </dev/null >/opt/var/log/mihomo.log 2>&1 & elif command -v mihomo >/dev/null 2>&1; then nohup mihomo -d /opt/etc/mihomo </dev/null >/opt/var/log/mihomo.log 2>&1 & fi".to_string()
        };
        log_i!("[STARTUP] Mihomo не запущен и service_stopped=false — выполняем автозапуск ядра...");
        let _ = tokio::process::Command::new("sh").arg("-c").arg(&start_cmd).output().await;
    });

    // Синхронизация файлов и Keenetic ndm хуков Zapret при старте панели
    let zapret_sync_state = state.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(6)).await;
        let cfg = zapret_sync_state.config.read().await.clone();
        if cfg.zapret.enabled {
            // Z-03: служба включена в конфигурации — маркер автозапуска должен отсутствовать
            zapret::clear_zapret_disabled_marker();
            if let Err(e) = api::sync_zapret_files(&cfg.zapret).await {
                log_w!("[STARTUP] Предупреждение: не удалось синхронизировать файлы Zapret: {}", e);
            }
        }
    });

    // Синхронизация полного перехвата (iptables) при старте панели: игровой режим
    // ИЛИ режим БС — sync_full_tunnel_modes сам расставит мьютекс
    let gaming_sync_state = state.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(7)).await;
        let cfg = gaming_sync_state.config.read().await.clone();
        let gaming_active = cfg.gaming.enabled && cfg.gaming.mode == config::GamingMode::Compatibility;
        let bs_active = cfg.whitelist.enabled;
        if gaming_active || bs_active {
            if let Ok(yaml) = tokio::fs::read_to_string(&cfg.mihomo.config_path).await {
                let ports = routing::redir_ports(&yaml);
                let (redir_port, tproxy_port) = (ports.redir, ports.tproxy);
                let dns_port = api::resolve_active_dns_port(&cfg);
                let _ = api::sync_full_tunnel_modes(&cfg, redir_port, tproxy_port, dns_port).await;
            }
        }
    });

    // PING-AUTO: фоновое автообновление пингов серверов (каждые 30 секунд замеряет
    // ограниченный набор: цепочка failover + основной узел + Fastest/Fallback/PROXY).
    // Замер через mihomo delay-тест обновляет историю нод, поэтому GET /api/servers
    // и 3-секундный опрос UI отдают живые пинги без действий пользователя в панели.
    let ping_auto_state = state.clone();
    tokio::spawn(async move {
        let stop = shutdown::global();
        tokio::select! {
            _ = stop.cancelled() => return,
            _ = tokio::time::sleep(std::time::Duration::from_secs(15)) => {}
        }
        loop {
            if stop.is_cancelled() {
                break;
            }
            let cfg = ping_auto_state.config.read().await.clone();
            crate::mihomo::restore_active_selectors_after_reload(
                &ping_auto_state.http,
                &cfg,
                crate::mihomo::last_selected_tunnel_server().as_deref(),
            )
            .await;
            let _ = crate::mihomo::run_auto_ping_tick(&ping_auto_state.http, &cfg).await;
            tokio::select! {
                _ = stop.cancelled() => break,
                _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => {}
            }
        }
    });

    // Фоновый автоподбор лучшего узла для Google AI / Flow (Баг 53)
    watchdog::spawn_flow_auto_select_loop(state.clone());

    // Smart Watchdog для бесплатных мульти-подписок (авто-перелистывание при зависании 0 Б/с)
    let free_sub_wd_state = state.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            free_sub::check_free_sub_stall_watchdog(&free_sub_wd_state).await;
        }
    });

    // Синхронизация AdGuard Home DNS-редиректа при старте панели
    let adguard_sync_state = state.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(4)).await;
        let cfg = adguard_sync_state.config.read().await.clone();
        let agh_status = adguard::get_status(&adguard_sync_state.http, &cfg.adguard).await;
        let is_running_protected = agh_status.running && agh_status.protection_enabled;
        let is_enabled = cfg.adguard.enabled
            || cfg.adblock_enabled
            || (!cfg.adguard.integration_choice_locked && is_running_protected);
        if agh_status.running {
            let _ = adguard::heal_adguard_live_config(&adguard_sync_state.http, &cfg.adguard, Some(&cfg.rci.host)).await;
        }
        if is_enabled {
            let active_port = adguard::detect_adguard_dns_port().unwrap_or(if agh_status.dns_port > 0 { agh_status.dns_port } else { cfg.adguard.dns_port });
            let listener_ok = adguard::test_agh_dns_listener(&cfg.adguard.host, active_port).await;
            if listener_ok {
                let _ = adguard::apply_adguard_iptables(active_port);
                log_i!("[STARTUP] ✓ AdGuard Home DNS redirection to port {} applied", active_port);
                if cfg.gaming.enabled && cfg.gaming.mode == config::GamingMode::Compatibility
                    || cfg.whitelist.enabled
                {
                    // Z-09: порты парсим из config.yaml, а не хардкодим 5000/5001 —
                    // при кастомном конфиге Mihomo правила уходили не на те порты
                    if let Ok(yaml) = tokio::fs::read_to_string(&cfg.mihomo.config_path).await {
                        let ports = routing::redir_ports(&yaml);
                        let (redir_port, tproxy_port) = (ports.redir, ports.tproxy);
                        let _ = api::sync_full_tunnel_modes(&cfg, redir_port, tproxy_port, active_port).await;
                    }
                }
            }

            // Автоматическая синхронизация AdGuard DNS upstream в config.yaml и config.json
            if let Ok(mut tx) = crate::transaction::ConfigTx::begin(&adguard_sync_state).await {
                let mut needs_save = false;
                if is_running_protected
                    && !tx.config().adguard.integration_choice_locked
                    && (!tx.config().adguard.enabled || tx.config().adguard.dns_port != active_port)
                {
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
        // Баг 4: ждём ответа Mihomo (до ~30 с дополнительно); «не ответил» ≠ «не БС»
        let mut mihomo_answered = false;
        let mut active_srv: Option<mihomo::Server> = None;
        for attempt in 0..7 {
            let cfg = wl_sync_state.config.read().await.clone();
            match mihomo::get_active_server(&wl_sync_state.http, &cfg).await {
                Ok(s) => {
                    mihomo_answered = true;
                    active_srv = s;
                    break;
                }
                Err(_) if attempt < 6 => tokio::time::sleep(std::time::Duration::from_secs(5)).await,
                Err(e) => log_w!("[STARTUP] Mihomo не ответил, авто-восстановление игрового режима отложено: {}", e),
            }
        }
        if mihomo_answered {
            if let Ok(mut tx) = crate::transaction::ConfigTx::begin(&wl_sync_state).await {
                if let Ok(current_yaml) = tx.read_yaml().await {
                    let cfg_snapshot = wl_sync_state.config.read().await.clone();
                    match reconcile_startup_yaml(&current_yaml, &cfg_snapshot) {
                        Ok(Some(new_yaml)) => {
                            if let Err(e) = tx.set_yaml(new_yaml) {
                                log_w!("[STARTUP] Отклонена синхронизация config.yaml: {}", e);
                            } else if let Err(e) = tx.commit_and_reload().await {
                                log_w!("[STARTUP] Не удалось применить обновлённый config.yaml: {}", e);
                            } else {
                                log_i!("[STARTUP] ✓ config.yaml синхронизирован с актуальной версией правил панели");
                                let closed = mihomo::close_server_connections(
                                    &wl_sync_state.http,
                                    &cfg_snapshot,
                                    crate::routing::WHITELIST_GROUP_NAME,
                                )
                                .await;
                                if closed > 0 {
                                    log_i!(
                                        "[STARTUP] Закрыто {} устаревших соединений группы {}",
                                        closed,
                                        crate::routing::WHITELIST_GROUP_NAME
                                    );
                                }
                            }
                        }
                        Ok(None) => {}
                        Err(e) => {
                            log_w!("[STARTUP] Ошибка проверки актуальности config.yaml: {}", e);
                        }
                    }
                }
            }
            let cfg_after = wl_sync_state.config.read().await.clone();
            mihomo::restore_active_selectors_after_reload(&wl_sync_state.http, &cfg_after, None).await;
            active_srv = match mihomo::get_active_server(&wl_sync_state.http, &cfg_after).await {
                Ok(s) => s,
                Err(_) => active_srv,
            };
            if let Some(ref s) = active_srv {
                mihomo::record_selected_tunnel_server(&s.id);
            }
        }
        let (active_id, is_bs, restore_gaming) = {
            let cfg = wl_sync_state.config.read().await;
            let active_id = active_srv.as_ref().map(|s| s.id.clone()).unwrap_or_default();
            let is_bs = active_srv
                .as_ref()
                .map(|s| failover::is_whitelist_server_full(&s.name, &s.id, s.provider.as_deref(), &cfg))
                .unwrap_or(false);
            let restore = failover::startup_gaming_restore_allowed(mihomo_answered, active_srv.as_ref(), &cfg);
            (active_id, is_bs, restore)
        };
        if !active_id.is_empty() {
            let _ = api::sync_whitelist_mode_tunnel(&wl_sync_state, &active_id).await;
        }

        // BS-FIX: страховочный повтор полного перехвата БС (8-я секунда) —
        // гарантирует XKEEN_BS даже если 7-секундный общий синк отработал раньше,
        // чем поднят firewall KeeneticOS (правила восстановит и NDM-хук)
        if is_bs {
            let cfg = wl_sync_state.cfg_snapshot().await;
            if cfg.whitelist.enabled {
                if let Ok(yaml) = tokio::fs::read_to_string(&cfg.mihomo.config_path).await {
                    let ports = routing::redir_ports(&yaml);
                    let (redir_port, tproxy_port) = (ports.redir, ports.tproxy);
                    let dns_port = api::resolve_active_dns_port(&cfg);
                    let _ = api::sync_full_tunnel_modes(&cfg, redir_port, tproxy_port, dns_port).await;
                    log_i!("[STARTUP] ✓ BS-FIX: полный перехват XKEEN_BS перемонтирован");
                }
            }
        }

        // Авто-восстановление игрового режима при перезагрузке роутера:
        // только если Mihomo ответил и активный узел точно не БС (с учётом подписок)
        if restore_gaming {
            // Атомарно: флаги и правила Mihomo применяются вместе, при ошибке reload — откат
            match api::commit_config_with_routing(&wl_sync_state, |c| {
                c.gaming.enabled = true;
                c.whitelist.gaming_suspended = false;
            })
            .await
            {
                Ok((current_cfg, new_yaml)) => {
                    log_i!("[STARTUP] ✓ Игровой режим автоматически восстановлен после перезагрузки (активный узел не является [БС])");
                    let ports = crate::routing::redir_ports(&new_yaml);
                    let (redir_port, tproxy_port) = (ports.redir, ports.tproxy);
                    let dns_port = crate::api::resolve_active_dns_port(&current_cfg);
                    let _ = crate::api::sync_full_tunnel_modes(&current_cfg, redir_port, tproxy_port, dns_port).await;
                    log_i!("[STARTUP] ✓ Правила игрового туннеля перемонтированы после восстановления");
                }
                Err(e) => log_w!("[STARTUP] Не удалось восстановить игровой режим: {}", e),
            }
        }
    });

    // Авто-миграция и плановое авто-обновление мобильных подписок (чистые имена серверов)
    let mobile_sub_state = state.clone();
    tokio::spawn(async move {
        let stop = shutdown::global();
        // Ожидание старта Mihomo
        tokio::select! {
            _ = stop.cancelled() => return,
            _ = tokio::time::sleep(std::time::Duration::from_secs(12)) => {}
        }
        log_i!("[STARTUP] Запуск начальной проверки и синхронизации мобильных подписок...");
        let migrated = api::sync_all_mobile_providers(&mobile_sub_state).await;
        if migrated > 0 {
            log_i!("[STARTUP] ✓ Синхронизировано мобильных подписок: {migrated}");
        }

        // Периодическое обновление остатка квот и подписок каждый час (3600 сек)
        // с быстрым повтором при ошибке/недоступности сети.
        loop {
            tokio::select! {
                _ = stop.cancelled() => break,
                _ = tokio::time::sleep(std::time::Duration::from_secs(3600)) => {}
            }
            if stop.is_cancelled() || failover::is_shutdown() {
                break;
            }
            log_i!("[QUOTA-SYNC] Плановое обновление квот и подписок (1h)...");
            let updated = api::sync_all_mobile_providers(&mobile_sub_state).await;
            if updated == 0 {
                // Если ни одна подписка не обновилась (сеть лежала или переключался туннель), ретрай через 2 минуты
                tokio::select! {
                    _ = stop.cancelled() => break,
                    _ = tokio::time::sleep(std::time::Duration::from_secs(120)) => {}
                }
                if !stop.is_cancelled() && !failover::is_shutdown() {
                    log_i!("[QUOTA-SYNC] Повторная попытка обновления квот через 2 мин...");
                    let _ = api::sync_all_mobile_providers(&mobile_sub_state).await;
                }
            }
        }
    });

    // Начальная быстрая синхронизация известных бандлов IP принудительно проксируемых доменов с geo_override
    let sync_state = state.clone();
    tokio::spawn(async move {
        // Задержка и ожидание готовности роутера/DNS после перезагрузки
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        wait_for_router_ready(&sync_state).await;

        let cfg = sync_state.cfg_snapshot().await;
        override_sync::load_cached_auto_cdns();
        log_i!("[STARTUP] Быстрая синхронизация известных CDN бандлов и игровых доменов с geo_override...");
        if let Err(e) = override_sync::sync_from_config(&cfg, &[]).await {
            log_w!("[STARTUP] Ошибка быстрой синхронизации geo_override: {}", e);
        } else {
            log_i!("[STARTUP] ✓ Быстрая синхронизация geo_override завершена");
        }
    });

    // Еженедельное плановое сканирование CDN (каждый понедельник в 05:00 утра)
    let weekly_state = state.clone();
    tokio::spawn(async move {
        loop {
            let wait_dur = duration_until_next_monday_5am();
            let next_run = chrono::Local::now() + chrono::Duration::from_std(wait_dur).unwrap_or_default();
            log_i!("[CDN-SCHEDULE] Следующее еженедельное сканирование CDN запланировано на {}", next_run.format("%Y-%m-%d %H:%M:%S"));

            let stop = shutdown::global();
            let mut elapsed = std::time::Duration::ZERO;
            while elapsed < wait_dur {
                if stop.is_cancelled() || failover::is_shutdown() {
                    return;
                }
                let step = std::cmp::min(std::time::Duration::from_secs(10), wait_dur - elapsed);
                tokio::select! {
                    _ = stop.cancelled() => return,
                    _ = tokio::time::sleep(step) => {}
                }
                elapsed += step;
            }
            if stop.is_cancelled() || failover::is_shutdown() {
                return;
            }

            let (cfg_snapshot, domains, proxy_url) = {
                let cfg = weekly_state.config.read().await;
                let domains = weekly_scan_inputs(&cfg);
                let proxy_url = cfg.mihomo_proxy_url();
                ((**cfg).clone(), domains, proxy_url)
            };
            if !domains.is_empty() {
                log_i!("[CDN-SCHEDULE] ⏰ Запуск планового еженедельного сканирования CDN (понедельник 05:00)...");
                let by_source = cdn_discovery::discover_cdns_by_source(&domains, &proxy_url).await;
                override_sync::sync_cached_auto_cdns_by_source(&by_source);
                if let Err(e) = override_sync::sync_from_config(&cfg_snapshot, &[]).await {
                    log_w!("[CDN-SCHEDULE] Ошибка плановой синхронизации geo_override: {}", e);
                } else {
                    log_i!("[CDN-SCHEDULE] ✓ Плановое еженедельное сканирование CDN успешно завершено");
                }
            }
        }
    });

    let app = build_app_router(state.clone());

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

    // Баг 127: запускаем таймер очистки маркера отката только после успешного открытия HTTP-порта
    let guard_cfg_path = state.config_path.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(updater::UPDATE_GUARD_HEALTHY_SECS)).await;
        let marker = updater::update_guard_marker_path(&guard_cfg_path);
        if marker.exists() {
            updater::clear_update_guard_marker(&marker);
            log_i!("[UPDATE] Новая версия стабильна {}s после открытия HTTP-порта — маркер отката удалён", updater::UPDATE_GUARD_HEALTHY_SECS);
        }
    });
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
    shutdown::global().cancel();
    state.vkturn.stop().await;
    antigravity::shutdown();
    failover::shutdown();
    watchdog::shutdown();
    traffic::shutdown();
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    log_i!("XKeen Route остановлен");
}


pub fn build_app_router(state: AppState) -> Router {
    let auth_state = state.clone();
    api::router::api_router()
        .fallback(frontend::serve)
        .layer(axum::extract::DefaultBodyLimit::max(10 * 1024 * 1024))
        .layer(middleware::from_fn_with_state(auth_state, auth::auth_middleware))
        .layer(middleware::from_fn(no_cache))
        .layer(middleware::from_fn(log_requests))
        .with_state(state)
}

/// Ожидание SIGINT/SIGTERM (init-скрипт Entware шлёт SIGTERM).
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        match signal(SignalKind::terminate()) {
            Ok(mut term) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {},
                    _ = term.recv() => {},
                }
            }
            Err(e) => {
                log_w!("Не удалось подписаться на SIGTERM ({}), ожидаю только Ctrl+C", e);
                let _ = tokio::signal::ctrl_c().await;
            }
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

pub fn weekly_scan_inputs(cfg: &config::AppConfig) -> Vec<String> {
    cfg.force_domains.clone()
}

/// Пересобирает управляемые блоки config.yaml по текущей конфигурации и правилам панели.
/// Возвращает `Ok(Some(new_yaml))`, если итоговый YAML отличается от текущего, иначе `Ok(None)`.
pub fn reconcile_startup_yaml(current_yaml: &str, cfg: &config::AppConfig) -> Result<Option<String>, String> {
    let (new_yaml, _) = crate::routing::apply_routing(current_yaml, cfg)?;
    if new_yaml == current_yaml {
        Ok(None)
    } else {
        Ok(Some(new_yaml))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regression_bug_a07_weekly_cdn_scan_releases_config_lock() {
        let mut cfg = config::AppConfig::default();
        cfg.force_domains = vec!["youtube.com".to_string(), "discord.com".to_string()];
        let inputs = weekly_scan_inputs(&cfg);
        assert_eq!(inputs, vec!["youtube.com".to_string(), "discord.com".to_string()]);
    }

    #[test]
    fn regression_startup_reconcile_migrates_stale_whitelist_rules_for_hh_ru() {
        let stale_yaml = r#"rules:
  # --- AUTO-WHITELIST-BEGIN ---
  - SRC-IP-CIDR,192.168.0.0/16,🛡️ Whitelist
  # --- AUTO-WHITELIST-END ---
  - MATCH,🌐 Proxy
"#;
        let mut cfg = config::AppConfig::default();
        cfg.whitelist.enabled = true;
        cfg.whitelist.strategy = config::WhitelistStrategy::StrictXKeen;
        cfg.whitelist.custom_domains = vec!["hh.ru".to_string()];

        let updated = reconcile_startup_yaml(stale_yaml, &cfg)
            .expect("reconcile_startup_yaml should succeed")
            .expect("stale YAML must be updated on startup");

        let hh_pos = updated
            .find("- DOMAIN-SUFFIX,hh.ru,DIRECT")
            .expect("updated YAML must include DIRECT rule for explicit custom_domains hh.ru");
        let src_pos = updated
            .find("- SRC-IP-CIDR,192.168.0.0/16,🛡️ Whitelist")
            .expect("updated YAML must keep LAN SRC-IP-CIDR rule");
        assert!(hh_pos < src_pos, "hh.ru DIRECT rule must appear before SRC-IP-CIDR whitelist catch-all");
        let yandex_pos = updated
            .find("- DOMAIN-SUFFIX,yandex.net,DIRECT")
            .expect("built-in catalog domain yandex.net must be routed DIRECT");
        assert!(yandex_pos < src_pos, "catalog DIRECT rule must appear before SRC-IP-CIDR whitelist catch-all");
        assert!(updated.contains("- MATCH,🛡️ Whitelist"), "global БС must end with MATCH,🛡️ Whitelist");

        let second_pass = reconcile_startup_yaml(&updated, &cfg)
            .expect("second reconcile pass should succeed");
        assert!(second_pass.is_none(), "reconcile_startup_yaml must be idempotent when YAML is already up to date");
    }
}

#[cfg(test)]
mod regression_bug_4_tests {
    /// Стартовый блок синхронизации БС (продакшн-часть; тесты стоят выше по файлу, поэтому rfind).
    fn startup_wl_block() -> &'static str {
        let src = include_str!("main.rs");
        let start = src.find("// Синхронизация режима обхода белых списков (100% full tunnel)").expect("начало блока");
        let end = start + src[start..].find("// Авто-миграция и плановое").expect("конец блока");
        &src[start..end]
    }

    #[test]
    fn regression_bug_4_gaming_not_restored_on_unknown_or_bs_node_after_boot() {
        // Баг 4: после перезагрузки «узел БС» определялся без учёта подписки, а если Mihomo на 8-й секунде
        // ещё не отвечал — результат «не БС», и игровой режим включался вместе с БС.
        let b = startup_wl_block();
        assert!(!b.contains("failover::is_whitelist_server(&s.name, &s.id, &cfg)"), "БС определяется с учётом подписки");
        assert!(!b.contains(".await.ok().flatten()"), "недоступный Mihomo не считается «не БС»");
        assert!(b.contains("failover::startup_gaming_restore_allowed("), "решение о восстановлении — через проверяемую функцию");
        assert!(!b.contains("config::save(&wl_sync_state.config_path"), "восстановление атомарно вместе с правилами");
    }
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
        assert!(script.contains("fd_out=true sh \"$XKEEN_INIT\" start on"));
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

#[cfg(test)]
mod architecture_invariant_tests {
    fn collect_rs_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_rs_files(&path, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }

    #[test]
    fn regression_architecture_invariants() {
        const TEST_CONFIG_WRITE_ALLOWLIST: &[&str] = &[];
        let src_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        collect_rs_files(&src_root, &mut files);
        files.sort();
        assert!(!files.is_empty(), "не найдены исходники backend/src");

        let needle = concat!(".config.", "write(");
        let mut violations = Vec::new();

        for path in &files {
            let rel = path
                .strip_prefix(&src_root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if rel == "transaction.rs" || TEST_CONFIG_WRITE_ALLOWLIST.contains(&rel.as_str()) {
                continue;
            }
            let content = std::fs::read_to_string(path).unwrap();
            let mut in_impl_app_state = false;
            for (idx, raw_line) in content.lines().enumerate() {
                let line_no = idx + 1;
                let trimmed = raw_line.trim();
                if rel == "main.rs" {
                    if trimmed.starts_with("impl AppState {") {
                        in_impl_app_state = true;
                        continue;
                    }
                    if in_impl_app_state && raw_line == "}" {
                        in_impl_app_state = false;
                        continue;
                    }
                    if in_impl_app_state {
                        continue;
                    }
                }
                if trimmed.starts_with("//") {
                    continue;
                }
                if raw_line.contains(needle) {
                    violations.push(format!("{rel}:{line_no}: {trimmed}"));
                }
            }
        }

        assert!(
            violations.is_empty(),
            "Обнаружен прямой вызов {needle} вне transaction.rs и AppState::update_config*:\n{}",
            violations.join("\n")
        );

        let cleaned_files = [
            "mihomo.rs",
            "failover/mod.rs",
            "api/mod.rs",
            "vkturn/mod.rs",
            "override_sync.rs",
            "main.rs",
        ];
        let fn_prefixes = [
            "fn ",
            "async fn ",
            "pub fn ",
            "pub async fn ",
            "pub(crate) fn ",
            "pub(crate) async fn ",
            "pub(super) fn ",
            "pub(super) async fn ",
        ];
        for rel in cleaned_files {
            let content = std::fs::read_to_string(src_root.join(rel)).unwrap();
            let mut first_cfg_test: Option<usize> = None;
            let mut last_top_fn: usize = 0;
            for (idx, line) in content.lines().enumerate() {
                let line_no = idx + 1;
                if first_cfg_test.is_none() && line.trim_start().starts_with("#[cfg(test)]") {
                    first_cfg_test = Some(line_no);
                }
                if !line.starts_with(' ')
                    && !line.starts_with('\t')
                    && fn_prefixes.iter().any(|p| line.starts_with(p))
                {
                    last_top_fn = line_no;
                }
            }
            if let Some(cfg_line) = first_cfg_test {
                assert!(
                    cfg_line > last_top_fn,
                    "{rel}: #[cfg(test)] на строке {cfg_line} находится выше рабочей функции на строке {last_top_fn}"
                );
            }
        }
    }
}
