use axum::extract::{Query, State};
use axum::response::{IntoResponse, Json, Response};
use serde::Deserialize;
use serde_json::json;

use super::{
    api_err, api_ok, compute_whitelist_mode_active, detect_installed_zapret1_version,
    detect_installed_zapret2_version, is_nfqws1_available, is_nfqws2_available,
    normalize_engine_choice,
};
use crate::{failover, log_i, mihomo, rci, AppState, VERSION};
/// GET /api/status — сводка: панель + роутер + активный сервер Mihomo.
pub async fn status(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();

    let (router_res, system_res, mihomo_ver, router_lan_ip) = tokio::join!(
        rci::get_version(&state.http, &cfg),
        rci::get_system(&state.http, &cfg),
        mihomo::get_version(&state.http, &cfg),
        rci::get_router_lan_ip(&state.http, &cfg)
    );
    let router = router_res.ok();
    let system = system_res.ok();

    let (active, priority_chain_pings) = match mihomo::get_servers(&state.http, &cfg, &cfg.failover.priority_chain).await {
        Ok(servers) => {
            let act = servers
                .iter()
                .find(|s| s.is_active && s.id != "Fastest" && s.id != "Fallback")
                .or_else(|| servers.iter().find(|s| s.is_active))
                .cloned()
                .map(|s| {
                    // PING-AUTO: живой замер из кэша панели важнее протухшей истории mihomo
                    let ping_ms = mihomo::cached_ping(&s.id).unwrap_or(s.ping_ms);
                    json!({
                        "id": s.id, "name": s.name, "ping_ms": ping_ms,
                        "protocol": s.protocol, "host": s.host, "port": s.port,
                        "provider": s.provider, "provider_name": s.provider_name,
                    })
                });
            let mut pings = std::collections::HashMap::new();
            for s in &servers {
                // PING-AUTO: и для карты пингов цепочки приоритетов тоже
                pings.insert(s.id.clone(), mihomo::cached_ping(&s.id).unwrap_or(s.ping_ms));
            }
            // Анти-регрессия: НЕ подменять реальный пинг на -1 (Таймаут),
            // чтобы панель всегда отображала фактический живой пинг из Mihomo.
            (act, pings)
        }
        Err(_) => (None, std::collections::HashMap::new()),
    };

    let zapret_engine = if normalize_engine_choice(&cfg.zapret.engine) == "v1" {
        "v1"
    } else if is_nfqws2_available() {
        "v2"
    } else if is_nfqws1_available() {
        "v1"
    } else {
        normalize_engine_choice(&cfg.zapret.engine)
    };
    let zapret_ver = if zapret_engine == "v2" {
        detect_installed_zapret2_version().await
    } else {
        detect_installed_zapret1_version().await
    };
    let zapret_label = if zapret_engine == "v2" {
        format!("Zapret 2 {zapret_ver}")
    } else {
        format!("Zapret 1 {zapret_ver}")
    };
    let zapret_installed = std::path::Path::new("/opt/etc/init.d/S51zapret").exists()
        && (is_nfqws2_available() || is_nfqws1_available());
    let service_stopped = crate::watchdog::is_service_stopped();
    let service_running = !service_stopped && mihomo_ver.is_some();
    let stack_memory = crate::system::get_stack_memory_info();
    let active_id = active.as_ref().and_then(|a| a.get("id")).and_then(|v| v.as_str()).unwrap_or("");
    let active_prov = active.as_ref().and_then(|a| a.get("provider")).and_then(|v| v.as_str()).unwrap_or("");
    let active_name = active.as_ref().and_then(|a| a.get("name")).and_then(|v| v.as_str()).unwrap_or("");
    // X-01: единый с sync_whitelist_mode_tunnel критерий режима БС
    let whitelist_mode_active = compute_whitelist_mode_active(
        &cfg,
        active_id,
        active_name,
        Some(active_prov).filter(|p| !p.is_empty()),
    );
    crate::traffic::set_whitelist_active(whitelist_mode_active);
    let active_ping = active.as_ref().and_then(|a| a.get("ping_ms")).and_then(|v| v.as_i64()).unwrap_or(-1);
    let current_failover_status = failover::current_live_status(active_id, active_name, active_ping, &cfg).await;

    let is_tier2 = !active_id.is_empty() && failover::is_whitelist_server_full(
        active_name,
        active_id,
        Some(active_prov).filter(|p| !p.is_empty()),
        &cfg,
    );
    let active_tier: u8 = if is_tier2 { 2 } else { 1 };
    let wl_episode_bytes = failover::get_whitelist_episode_bytes();

    api_ok(json!({
        "version": VERSION,
        "config_path": state.config_path.display().to_string(),
        "router": router,
        "router_ip": router_lan_ip,
        "system": system,
        "mihomo_version": mihomo_ver,
        "service_running": service_running,
        "service_stopped": service_stopped,
        "active_server": active,
        "stack_memory": stack_memory,
        "whitelist_mode_active": whitelist_mode_active,
        "whitelist_servers": cfg.whitelist_servers,
        "normal_servers": cfg.normal_servers,
        "whitelist_providers": cfg.whitelist_providers,
        "server_aliases": cfg.server_aliases,
        "mihomo": { "host": cfg.mihomo.host, "port": cfg.mihomo.port },
        "rci": { "host": cfg.rci.host, "port": cfg.rci.port },
        "failover": {
            "enabled": cfg.failover.enabled,
            "ping_threshold_ms": cfg.failover.ping_threshold_ms,
            "priority_server": if !cfg.failover.priority_server.is_empty() {
                cfg.failover.priority_server.clone()
            } else {
                cfg.failover.priority_chain.first().cloned().unwrap_or_default()
            },
            "priority_chain": cfg.failover.priority_chain,
            "priority_chain_pings": priority_chain_pings,
            "bs_priority_chain": cfg.failover.bs_priority_chain,
            "whitelist_detector": cfg.failover.whitelist_detector,
            "whitelist_detector_status": failover::get_whitelist_detector_status().await,
            "last_good_regular_vps": cfg.failover.last_good_regular_vps,
            "auto_restore_priority": cfg.failover.auto_restore_priority,
            "interval_secs": cfg.failover.interval_secs,
            "canary_duration_secs": cfg.failover.canary_duration_secs,
            "device_failover_enabled": cfg.failover.device_failover_enabled,
            "best_ping_enabled": cfg.failover.best_ping_enabled,
            "best_ping_pool_size": cfg.failover.best_ping_pool_size,
            "best_ping_hysteresis_ms": cfg.failover.best_ping_hysteresis_ms,
            "smart_failover_enabled": cfg.failover.smart_failover_enabled,
            "smart_failover_canary_duration_secs": cfg.failover.smart_failover_canary_duration_secs,
            "provider_chain": cfg.failover.provider_chain,
            "active_tier": active_tier,
            "whitelist_episode_bytes": wl_episode_bytes,
            "canary": failover::canary_status().await,
            "last_test": failover::canary_last_result().await,
            "quarantine_enabled": cfg.failover.quarantine_enabled,
            "zombie_servers": if cfg.failover.quarantine_enabled { failover::get_zombie_servers().await } else { Vec::new() },
            "quarantine": if cfg.failover.quarantine_enabled { failover::quarantine_status().await } else { Vec::new() },
            "current_status": current_failover_status,
        },
        "refresh_interval_sec": cfg.refresh_interval_sec,
        "adblock_enabled": cfg.adblock_enabled,
        "zapret": {
            "engine": zapret_engine,
            "version": zapret_ver,
            "label": zapret_label,
            "installed": zapret_installed,
            "running": cfg.zapret.enabled && !service_stopped,
            "v1_installed": is_nfqws1_available(),
            "v2_installed": is_nfqws2_available(),
            "update_available": cfg.zapret.update_available
                && cfg.zapret.latest_version.as_deref().is_some_and(|lat| crate::updater::is_newer(lat, &zapret_ver)),
            "latest_version": cfg.zapret.latest_version,
        },
        "gaming": {
            "enabled": cfg.gaming.enabled,
            "mode": cfg.gaming.mode,
            "smart_mode": cfg.gaming.smart_mode,
            "smart_idle_timeout_mins": cfg.gaming.smart_idle_timeout_mins,
        },
    }))
    .into_response()
}

/// GET /api/system/metrics — сверхлёгкий эндпоинт для обновления метрик каждую секунду: CPU роутера, RAM роутера, RAM/CPU процесса панели.
pub async fn get_system_metrics(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    match rci::get_system(&state.http, &cfg).await {
        Ok(stats) => api_ok(serde_json::to_value(stats).unwrap_or_default()).into_response(),
        Err(e) => api_err(e).into_response(),
    }
}

/// GET /api/system/processes — снимок диспетчера задач (ресурсы CPU/RAM/Swap + список процессов)
pub async fn get_system_processes(State(_state): State<AppState>) -> Response {
    let snapshot = crate::system::get_task_manager_snapshot().await;
    api_ok(serde_json::to_value(snapshot).unwrap_or_default()).into_response()
}

/// GET /api/system/stack-memory — физическая память (RSS) ключевых процессов стека
pub async fn get_stack_memory(State(_state): State<AppState>) -> Response {
    let stack_info = crate::system::get_stack_memory_info();
    api_ok(serde_json::to_value(stack_info).unwrap_or_default()).into_response()
}

/// POST /api/system/processes/kill — безопасное завершение процесса роутера
pub async fn kill_process(
    State(_state): State<AppState>,
    Json(body): Json<crate::system::KillRequest>,
) -> Response {
    match crate::system::kill_process_by_pid(body.pid, body.signal.as_deref()).await {
        Ok(msg) => api_ok(json!({ "pid": body.pid, "message": msg })).into_response(),
        Err(err) => api_err(err).into_response(),
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct CreateSwapRequest {
    #[serde(default = "default_swap_size_mb")]
    pub size_mb: u64,
}

fn default_swap_size_mb() -> u64 {
    512
}

/// GET /api/system/swap/status — получение текущего состояния Swap
pub async fn get_swap_status() -> Response {
    let status = tokio::task::spawn_blocking(crate::system::get_swap_status)
        .await
        .unwrap_or_else(|_| crate::system::get_swap_status());
    api_ok(serde_json::to_value(status).unwrap_or_default()).into_response()
}

/// POST /api/system/swap/create — создание и активация файла подкачки на /opt
pub async fn create_swap(Json(payload): Json<CreateSwapRequest>) -> Response {
    let size_mb = payload.size_mb;
    let res = crate::system::create_and_enable_swap(size_mb).await;

    match res {
        Ok(status) => api_ok(serde_json::to_value(status).unwrap_or_default()).into_response(),
        Err(err_msg) => api_err(err_msg).into_response(),
    }
}

/// POST /api/system/swap/remove — безопасное отключение и удаление Swap
pub async fn remove_swap() -> Response {
    let res = tokio::task::spawn_blocking(crate::system::disable_and_remove_swap)
        .await
        .map_err(|e| format!("Внутренняя ошибка задачи: {}", e))
        .and_then(|r| r);

    match res {
        Ok(status) => api_ok(serde_json::to_value(status).unwrap_or_default()).into_response(),
        Err(err_msg) => api_err(err_msg).into_response(),
    }
}





// =====================================================================
// 1. GET /api/system/diagnostics — глубокая системная диагностика
// =====================================================================
pub async fn get_system_diagnostics(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();

    // Mihomo
    let mihomo_installed = std::path::Path::new("/opt/sbin/mihomo").exists()
        || (!cfg.system.mihomo_bin.is_empty() && std::path::Path::new(&cfg.system.mihomo_bin).exists());
    let mihomo_ver = crate::mihomo::get_version(&state.http, &cfg).await;
    let service_stopped = crate::watchdog::is_service_stopped();
    let mihomo_running = !service_stopped && mihomo_ver.is_some();
    let tun_active = crate::system::is_tun_active();

    // Zapret
    let z_v1 = is_nfqws1_available();
    let z_v2 = is_nfqws2_available();
    let mut z_bins = Vec::new();
    if z_v1 { z_bins.push("nfqws1".to_string()); }
    if z_v2 { z_bins.push("nfqws2".to_string()); }
    if std::path::Path::new("/opt/sbin/tpws").exists() { z_bins.push("tpws".to_string()); }
    let zapret_installed = std::path::Path::new("/opt/etc/init.d/S51zapret").exists() && (z_v1 || z_v2);
    let zapret_running = cfg.zapret.enabled && !service_stopped;
    let zapret_engine = if normalize_engine_choice(&cfg.zapret.engine) == "v1" {
        "v1"
    } else if z_v2 {
        "v2"
    } else {
        "v1"
    };
    let zapret_ver = if zapret_engine == "v2" {
        detect_installed_zapret2_version().await
    } else {
        detect_installed_zapret1_version().await
    };

    // AdGuard
    let agh_installed = std::path::Path::new("/opt/bin/AdGuardHome").exists()
        || std::path::Path::new("/opt/etc/AdGuardHome/AdGuardHome.yaml").exists()
        || std::path::Path::new("/opt/etc/AdGuardHome.yaml").exists();
    let agh_status = crate::adguard::get_status(&state.http, &cfg.adguard).await;
    let agh_running = agh_status.running;
    let agh_port = if agh_status.dns_port > 0 {
        agh_status.dns_port
    } else {
        cfg.adguard.dns_port
    };
    let agh_health = crate::adguard::get_health(&state.http, &cfg.adguard, Some(&cfg.rci.host)).await;
    let agh_loop = agh_health.loop_risk;

    // Порты
    let mihomo_dns_port = crate::routing::mihomo_dns_port(&cfg);
    let ports = crate::system::detect_listening_ports(agh_port, cfg.mihomo.mixed_port, mihomo_dns_port);

    // Iptables
    let iptables_diag = crate::system::IptablesDiagnostics {
        redirect_active: crate::adguard::is_adguard_iptables_active(agh_port),
        adguard_chain_active: crate::adguard::is_adguard_iptables_active(agh_port),
        zapret_chain_active: zapret_running,
    };

    // Хранилище /opt
    let storage_diag = crate::system::check_opt_storage();

    // Память и аптайм
    let (mem_total, mem_free, _, _) = crate::system::read_meminfo();
    let sys_resources = (mem_total / 1024, (mem_total.saturating_sub(mem_free)) / 1024);
    let uptime_sec = match std::fs::read_to_string("/proc/uptime") {
        Ok(s) => s.split_whitespace().next().and_then(|u| u.parse::<f64>().ok()).map(|u| u as u64).unwrap_or(0),
        Err(_) => 0,
    };

    let overall_status = if !storage_diag.opt_rw || agh_loop || (mihomo_installed && !mihomo_running && !service_stopped) {
        if !storage_diag.opt_rw || (mihomo_installed && !mihomo_running && !service_stopped) {
            "error"
        } else {
            "degraded"
        }
    } else {
        "healthy"
    };

    let diag = crate::system::SystemDiagnostics {
        status: overall_status.to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        components: crate::system::ComponentDiagnostics {
            mihomo: crate::system::MihomoComponentDiag {
                installed: mihomo_installed,
                running: mihomo_running,
                pid: None,
                version: mihomo_ver,
                tun_active,
            },
            zapret: crate::system::ZapretComponentDiag {
                installed: zapret_installed,
                running: zapret_running,
                engine: zapret_engine.to_string(),
                version: zapret_ver,
                binaries_found: z_bins,
                missing: vec![],
            },
            adguard: crate::system::AdguardComponentDiag {
                installed: agh_installed,
                running: agh_running,
                pid: None,
                dns_port: agh_port,
                loop_detected: agh_loop,
            },
        },
        listening_ports: ports,
        iptables: iptables_diag,
        storage: storage_diag,
        system_summary: crate::system::SystemSummaryDiagnostics {
            ram_total_mb: sys_resources.0,
            ram_used_mb: sys_resources.1,
            cpu_total_percent: 0.0,
            uptime_seconds: uptime_sec,
        },
    };

    api_ok(serde_json::to_value(diag).unwrap_or_default()).into_response()
}

// =====================================================================
// 2. GET /api/system/logs/recent — кольцевой буфер последних логов
// =====================================================================
#[derive(Debug, Deserialize)]
pub struct RecentLogsQuery {
    pub level: Option<String>,
    pub limit: Option<usize>,
}

pub async fn get_recent_logs(Query(q): Query<RecentLogsQuery>) -> Response {
    let limit = q.limit.unwrap_or(50);
    let entries = crate::logger::get_recent_logs(q.level.as_deref(), limit);
    api_ok(json!({
        "total": entries.len(),
        "entries": entries,
    })).into_response()
}

// ---------- Логи ----------

#[derive(Deserialize)]
pub struct LogsQuery {
    pub lines: Option<usize>,
}

/// GET /api/logs?lines=500 — хвост журнала.
/// Файловый I/O — в spawn_blocking, чтобы не блокировать tokio-воркеры.
pub async fn logs_tail(Query(q): Query<LogsQuery>) -> Response {
    let lines = q.lines.unwrap_or(500);
    match tokio::task::spawn_blocking(move || crate::logger::tail(lines)).await {
        Ok(Ok(text)) => api_ok(json!({
            "text": text,
            "path": crate::logger::path()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
        })),
        Ok(Err(e)) => api_err(e),
        Err(e) => api_err(format!("internal: {e}")),
    }
}

/// GET /api/logs/download — скачать весь журнал (text/plain).
pub async fn logs_download() -> impl IntoResponse {
    let text = tokio::task::spawn_blocking(crate::logger::read_all)
        .await
        .unwrap_or_else(|e| Err(format!("internal: {e}")))
        .unwrap_or_default();
    (
        [
            ("Content-Type", "text/plain; charset=utf-8"),
            (
                "Content-Disposition",
                "attachment; filename=\"xkeen-route.log\"",
            ),
        ],
        text,
    )
        .into_response()
}

/// GET /api/logs/ws — живой поток журнала (WebSocket).
pub async fn logs_ws(
    ws: axum::extract::ws::WebSocketUpgrade,
    Query(q): Query<LogsQuery>,
) -> Response {
    ws.on_upgrade(move |socket| async move {
        ws_logs_stream(socket, q.lines.unwrap_or(200)).await
    })
}

async fn ws_logs_stream(mut socket: axum::extract::ws::WebSocket, history_lines: usize) {
    // 1. Сначала — хвост истории.
    if let Ok(text) = crate::logger::tail(history_lines) {
        for line in text.lines() {
            if socket.send(axum::extract::ws::Message::text(line)).await.is_err() {
                return;
            }
        }
    }
    // 2. Затем — живой поток новых строк.
    let Some(mut rx) = crate::logger::subscribe() else {
        let _ = socket.send(axum::extract::ws::Message::text("(лог не инициализирован)")).await;
        return;
    };
    loop {
        tokio::select! {
            client_msg = socket.recv() => {
                match client_msg {
                    Some(Ok(axum::extract::ws::Message::Ping(payload))) => {
                        if socket.send(axum::extract::ws::Message::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(axum::extract::ws::Message::Close(_))) | None => {
                        break;
                    }
                    _ => {}
                }
            }
            log_res = rx.recv() => {
                match log_res {
                    Ok(line) => {
                        if socket.send(axum::extract::ws::Message::text(line)).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        let _ = socket
                            .send(axum::extract::ws::Message::text(format!("…пропущено {n} строк…")))
                            .await;
                    }
                    Err(_) => break,
                }
            }
        }
    }
}

/// POST /api/logs/clear — очистить журнал.
pub async fn logs_clear() -> Response {
    match crate::logger::clear() {
        Ok(_) => {
            log_i!("Журнал очищен");
            api_ok(json!({ "cleared": true }))
        }
        Err(e) => api_err(e),
    }
}

// ==================== СТРИМИНГ ЛОГОВ ЯДРА MIHOMO =============

/// GET /api/logs/mihomo — получение хвоста логов ядра Mihomo
pub async fn mihomo_logs_tail(
    State(state): State<AppState>,
    Query(q): Query<LogsQuery>,
) -> Response {
    let cfg = state.config.read().await.clone();
    let lines_count = q.lines.unwrap_or(200);

    let log_path = std::path::Path::new(&cfg.mihomo.log_path);
    if log_path.exists() {
        if let Ok(mut f) = tokio::fs::File::open(log_path).await {
            if let Ok(meta) = f.metadata().await {
                use tokio::io::{AsyncReadExt, AsyncSeekExt, SeekFrom};
                let file_len = meta.len();
                let max_bytes = 256 * 1024u64;
                let seek_pos = file_len.saturating_sub(max_bytes);
                if seek_pos > 0 {
                    let _ = f.seek(SeekFrom::Start(seek_pos)).await;
                }
                let mut buf = Vec::with_capacity((file_len - seek_pos) as usize);
                if f.read_to_end(&mut buf).await.is_ok() {
                    let content = String::from_utf8_lossy(&buf);
                    let mut lines: Vec<&str> = content.lines().collect();
                    if seek_pos > 0 && !lines.is_empty() {
                        lines.remove(0);
                    }
                    let start = lines.len().saturating_sub(lines_count);
                    let tail = lines[start..].join("\n");
                    return api_ok(json!({
                        "text": tail,
                        "source": "file",
                        "path": log_path.display().to_string()
                    }));
                }
            }
        }
    }

    let url = format!("{}/logs?level=info", cfg.mihomo_url());
    let mut req = state.http.get(&url);
    if !cfg.mihomo.secret.is_empty() {
        req = req.header("Authorization", format!("Bearer {}", cfg.mihomo.secret));
    }

    match req.timeout(std::time::Duration::from_secs(2)).send().await {
        Ok(resp) => {
            let body = resp.text().await.unwrap_or_default();
            api_ok(json!({
                "text": body,
                "source": "api",
                "path": url
            }))
        }
        Err(_) => api_ok(json!({
            "text": "(Журнал ядра Mihomo пуст или пишется в системный консольный лог)",
            "source": "none",
            "path": ""
        })),
    }
}

/// POST /api/logs/mihomo/clear — усечение файла журнала Mihomo
pub async fn mihomo_logs_clear(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    let log_path = std::path::Path::new(&cfg.mihomo.log_path);
    if log_path.exists() {
        if let Err(e) = tokio::fs::write(log_path, b"").await {
            return api_err(format!("Не удалось очистить лог Mihomo: {e}"));
        }
    }
    log_i!("Журнал Mihomo очищен пользователем");
    api_ok(json!({ "cleared": true }))
}

// ==================== ТЕСТ УВЕДОМЛЕНИЙ (TELEGRAM / WEBHOOK) =============

#[derive(Deserialize)]
pub struct TestNotificationRequest {
    pub telegram_bot_token: Option<String>,
    pub telegram_chat_id: Option<String>,
    pub webhook_url: Option<String>,
}

/// POST /api/notifications/test
pub async fn test_notification(
    State(state): State<AppState>,
    Json(body): Json<TestNotificationRequest>,
) -> Response {
    let cfg = state.config.read().await.clone();
    let bot_token = match body.telegram_bot_token {
        Some(ref t) if !t.trim().is_empty() && t.trim() != "******" => t.trim().to_string(),
        _ => cfg.notifications.telegram_bot_token.clone(),
    };
    let chat_id = match body.telegram_chat_id {
        Some(ref c) if !c.trim().is_empty() => c.trim().to_string(),
        _ => cfg.notifications.telegram_chat_id.clone(),
    };
    let webhook = match body.webhook_url {
        Some(ref w) if !w.trim().is_empty() => w.trim().to_string(),
        _ => cfg.notifications.webhook_url.clone(),
    };
    let proxy_url = cfg.mihomo_proxy_url();

    let mut results = Vec::new();

    if !bot_token.trim().is_empty() || !chat_id.trim().is_empty() {
        if bot_token.trim().is_empty() {
            results.push("Telegram: ❌ Не указан Bot Token".to_string());
        } else if chat_id.trim().is_empty() {
            results.push("Telegram: ❌ Не указан Chat ID".to_string());
        } else {
            let test_msg = "<b>🔔 Тестовое уведомление XKeen Route</b>\n\nСвязь с Telegram Bot API успешно установлена!\nВы будете получать оповещения при сбоях и переключениях серверов Failover.";
            match crate::notifications::send_telegram_with_proxy(
                &state.http,
                Some(&proxy_url),
                &bot_token,
                &chat_id,
                test_msg,
            ).await {
                Ok(_) => results.push("Telegram: ✓ Сообщение успешно отправлено".to_string()),
                Err(e) => results.push(format!("Telegram: ❌ {}", e)),
            }
        }
    }

    if !webhook.is_empty() {
        match crate::notifications::send_webhook(
            &state.http,
            &webhook,
            "test",
            "Тестовое уведомление",
            "Тестовая проверка Webhook из веб-панели XKeen Route",
        ).await {
            Ok(_) => results.push("Webhook: ✓ Запрос успешно доставлен".to_string()),
            Err(e) => results.push(format!("Webhook: ❌ {}", e)),
        }
    }

    if results.is_empty() {
        return api_err("Укажите Telegram Bot Token и Chat ID или Webhook URL для теста");
    }

    let has_error = results.iter().any(|r| r.contains('❌'));
    let msg = results.join("\n");
    if has_error {
        api_err(msg)
    } else {
        api_ok(json!({ "results": results, "message": msg }))
    }
}
