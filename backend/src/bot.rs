//! Telegram Bot Daemon для управления роутером Keenetic через XKeen Route.
//! Поддерживает Long Polling через локальный прокси Mihomo (обход блокировок ТСПУ),
//! авторизацию по Chat ID, интерактивные инлайн-клавиатуры и оперативные команды.

use std::time::Duration;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::time::sleep;

use crate::{log_i, log_w, AppState};

#[derive(Deserialize, Debug, Clone)]
pub struct TelegramUpdate {
    pub update_id: i64,
    pub message: Option<TelegramMessage>,
    pub callback_query: Option<TelegramCallbackQuery>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct TelegramMessage {
    pub message_id: i64,
    pub chat: TelegramChat,
    pub text: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct TelegramChat {
    pub id: i64,
}

#[derive(Deserialize, Debug, Clone)]
pub struct TelegramCallbackQuery {
    pub id: String,
    pub message: Option<TelegramMessage>,
    pub data: Option<String>,
}

#[derive(Serialize)]
struct InlineKeyboardButton {
    text: String,
    callback_data: String,
}

#[derive(Serialize)]
struct InlineKeyboardMarkup {
    inline_keyboard: Vec<Vec<InlineKeyboardButton>>,
}

/// Запуск фонового демона Telegram-бота
pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        // Задержка перед стартом, чтобы Mihomo и интерфейсы роутера успели подняться
        sleep(Duration::from_secs(4)).await;
        run_bot_loop(state).await;
    });
}

async fn get_bot_client(proxy_url: &str) -> Option<reqwest::Client> {
    let p_trimmed = proxy_url.trim();
    if !p_trimmed.is_empty() {
        if let Ok(proxy) = reqwest::Proxy::all(p_trimmed) {
            if let Ok(c) = reqwest::Client::builder()
                .proxy(proxy)
                .timeout(Duration::from_secs(35))
                .build()
            {
                return Some(c);
            }
        }
    }
    reqwest::Client::builder()
        .timeout(Duration::from_secs(35))
        .build()
        .ok()
}

/// Проверка, активен ли процесс Zapret (nfqws или nfqws2)
async fn is_zapret_running() -> bool {
    #[cfg(unix)]
    {
        if let Ok(out) = tokio::process::Command::new("sh")
            .arg("-c")
            .arg("pidof nfqws2 2>/dev/null || pidof nfqws 2>/dev/null")
            .output()
            .await
        {
            return !out.stdout.is_empty();
        }
    }
    false
}

/// Управление службой Zapret через init-скрипт
async fn zapret_toggle_action(action: &str) -> Result<(), String> {
    #[cfg(unix)]
    {
        let script = "/opt/etc/init.d/S51zapret";
        let out = tokio::process::Command::new("sh")
            .arg(script)
            .arg(action)
            .output()
            .await
            .map_err(|e| e.to_string())?;
        if out.status.success() {
            return Ok(());
        } else {
            return Err(String::from_utf8_lossy(&out.stderr).to_string());
        }
    }
    #[cfg(not(unix))]
    {
        let _ = action;
        Ok(())
    }
}

/// Основной цикл Long Polling
async fn run_bot_loop(state: AppState) {
    let mut offset: i64 = 0;
    let mut commands_registered = false;

    log_i!("[BOT] Демон Telegram-бота инициализирован");

    loop {
        let (token, target_chat_id, enabled, proxy_url) = {
            let cfg = state.config.read().await;
            (
                cfg.notifications.telegram_bot_token.trim().to_string(),
                cfg.notifications.telegram_chat_id.trim().to_string(),
                cfg.notifications.telegram_enabled,
                cfg.mihomo_proxy_url(),
            )
        };

        if !enabled || token.is_empty() || target_chat_id.is_empty() {
            // Бот не настроен или выключен в настройках — ждём перед повторной проверкой
            sleep(Duration::from_secs(10)).await;
            continue;
        }

        let http = match get_bot_client(&proxy_url).await {
            Some(c) => c,
            None => {
                sleep(Duration::from_secs(5)).await;
                continue;
            }
        };

        // Однократная регистрация списка команд в Telegram (кнопка «Меню»)
        if !commands_registered {
            if register_commands(&http, &token).await.is_ok() {
                commands_registered = true;
                log_i!("[BOT] Меню команд успешно зарегистрировано в Telegram Bot API");
            }
        }

        // Запрос обновлений методом Long Polling (getUpdates)
        let updates_url = format!(
            "https://api.telegram.org/bot{}/getUpdates?offset={}&timeout=25&allowed_updates=[\"message\",\"callback_query\"]",
            token, offset
        );

        let res = http
            .get(&updates_url)
            .header("User-Agent", "xkeen-route")
            .send()
            .await;

        match res {
            Ok(resp) => {
                if resp.status().is_success() {
                    if let Ok(body) = resp.json::<Value>().await {
                        if let Some(list) = body.get("result").and_then(|r| r.as_array()) {
                            for item in list {
                                if let Ok(update) = serde_json::from_value::<TelegramUpdate>(item.clone()) {
                                    if update.update_id >= offset {
                                        offset = update.update_id + 1;
                                    }
                                    handle_update(&state, &http, &token, &target_chat_id, update).await;
                                }
                            }
                        }
                    }
                } else {
                    let st = resp.status();
                    if st == reqwest::StatusCode::UNAUTHORIZED {
                        log_w!("[BOT] Неверный токен бота (401 Unauthorized)");
                        sleep(Duration::from_secs(30)).await;
                    } else {
                        sleep(Duration::from_secs(5)).await;
                    }
                }
            }
            Err(e) => {
                log_w!("[BOT] Ошибка соединения с Telegram API: {}. Повтор...", e);
                sleep(Duration::from_secs(4)).await;
            }
        }
    }
}

/// Автоматическая регистрация команд в синей кнопке «Меню» Telegram
async fn register_commands(http: &reqwest::Client, token: &str) -> Result<(), String> {
    let url = format!("https://api.telegram.org/bot{}/setMyCommands", token);
    let commands = json!({
        "commands": [
            { "command": "start", "description": "📱 Главное меню и пульт управления" },
            { "command": "status", "description": "📊 Статус роутера, ОЗУ, CPU и серверов" },
            { "command": "servers", "description": "🔄 Выбор активного VPN-сервера" },
            { "command": "ping", "description": "⚡ Замер пинга до узлов" },
            { "command": "zapret", "description": "🛡️ Управление DPI-обходом (YouTube/Discord)" },
            { "command": "restart", "description": "🔁 Перезапуск службы XKeen" },
            { "command": "help", "description": "ℹ️ Справка и возможности" }
        ]
    });

    let res = http
        .post(&url)
        .header("User-Agent", "xkeen-route")
        .json(&commands)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if res.status().is_success() {
        Ok(())
    } else {
        Err(format!("HTTP {}", res.status()))
    }
}

/// Обработка входящего обновления
async fn handle_update(
    state: &AppState,
    http: &reqwest::Client,
    token: &str,
    target_chat_id: &str,
    update: TelegramUpdate,
) {
    // 1. Обработка Callback Query (клики по кнопкам под сообщениями)
    if let Some(cb) = update.callback_query {
        let chat_id_str = cb
            .message
            .as_ref()
            .map(|m| m.chat.id.to_string())
            .unwrap_or_default();

        if !is_authorized(&chat_id_str, target_chat_id) {
            let _ = answer_callback(http, token, &cb.id, Some("⛔ Доступ запрещён")).await;
            return;
        }

        if let Some(data) = cb.data {
            handle_callback(state, http, token, cb.message, &cb.id, &data).await;
        }
        return;
    }

    // 2. Обработка входящего текстового сообщения
    if let Some(msg) = update.message {
        let chat_id_str = msg.chat.id.to_string();
        if !is_authorized(&chat_id_str, target_chat_id) {
            log_w!("[BOT] Попытка неавторизованного доступа от Chat ID: {}", chat_id_str);
            let _ = send_msg(
                http,
                token,
                &chat_id_str,
                "⛔ <b>Доступ запрещён</b>\nЭтот бот привязан к персональному роутеру Keenetic. Управление разрешено только владельцу.",
                None,
            ).await;
            return;
        }

        if let Some(text) = msg.text {
            handle_text_command(state, http, token, &chat_id_str, text.trim()).await;
        }
    }
}

/// Проверка совпадения Chat ID с доверенным
pub fn is_authorized(chat_id: &str, target_chat_id: &str) -> bool {
    chat_id.trim() == target_chat_id.trim()
}

/// Обработчик текстовых команд
async fn handle_text_command(
    state: &AppState,
    http: &reqwest::Client,
    token: &str,
    chat_id: &str,
    text: &str,
) {
    let lower = text.to_lowercase();
    let cmd = lower.split_whitespace().next().unwrap_or("");

    match cmd {
        "/start" | "/menu" | "/help" => {
            send_dashboard(state, http, token, chat_id, None).await;
        }
        "/status" => {
            let status_text = generate_status_report(state).await;
            let kb = InlineKeyboardMarkup {
                inline_keyboard: vec![
                    vec![
                        InlineKeyboardButton { text: "🔄 Обновить".into(), callback_data: "cb:status".into() },
                        InlineKeyboardButton { text: "⚡ Пинг".into(), callback_data: "cb:ping".into() },
                    ],
                    vec![
                        InlineKeyboardButton { text: "◀️ В главное меню".into(), callback_data: "cb:menu".into() },
                    ],
                ],
            };
            let _ = send_msg(http, token, chat_id, &status_text, Some(json!(kb))).await;
        }
        "/servers" => {
            send_servers_menu(state, http, token, chat_id, None).await;
        }
        "/ping" => {
            let ping_text = generate_ping_report(state).await;
            let kb = InlineKeyboardMarkup {
                inline_keyboard: vec![
                    vec![
                        InlineKeyboardButton { text: "⚡ Повторить замер".into(), callback_data: "cb:ping".into() },
                        InlineKeyboardButton { text: "🔄 Сменить сервер".into(), callback_data: "cb:servers".into() },
                    ],
                    vec![
                        InlineKeyboardButton { text: "◀️ В меню".into(), callback_data: "cb:menu".into() },
                    ],
                ],
            };
            let _ = send_msg(http, token, chat_id, &ping_text, Some(json!(kb))).await;
        }
        "/zapret" => {
            send_zapret_menu(state, http, token, chat_id, None).await;
        }
        "/restart" => {
            let kb = InlineKeyboardMarkup {
                inline_keyboard: vec![
                    vec![
                        InlineKeyboardButton { text: "⚠️ Да, перезапустить".into(), callback_data: "cb:restart_confirm".into() },
                        InlineKeyboardButton { text: "❌ Отмена".into(), callback_data: "cb:menu".into() },
                    ],
                ],
            };
            let _ = send_msg(
                http,
                token,
                chat_id,
                "❓ <b>Подтверждение перезапуска</b>\nВы действительно хотите перезапустить службы проксирования XKeen/Mihomo?",
                Some(json!(kb)),
            ).await;
        }
        _ => {
            let clean_domain = crate::config::normalize_domain(text);
            if crate::config::is_valid_domain(&clean_domain) {
                handle_domain_analysis(state, http, token, chat_id, &clean_domain).await;
            } else {
                let _ = send_msg(
                    http,
                    token,
                    chat_id,
                    "💡 <b>Команда не распознана</b>\nИспользуйте меню или отправьте имя домена (например, <code>instagram.com</code>) для проверки маршрутизации.",
                    None,
                ).await;
            }
        }
    }
}

/// Обработчик нажатий на инлайн-кнопки
async fn handle_callback(
    state: &AppState,
    http: &reqwest::Client,
    token: &str,
    msg_opt: Option<TelegramMessage>,
    cb_id: &str,
    data: &str,
) {
    let msg = match msg_opt {
        Some(m) => m,
        None => return,
    };
    let chat_id = msg.chat.id.to_string();
    let msg_id = msg.message_id;

    if data == "cb:menu" {
        let _ = answer_callback(http, token, cb_id, None).await;
        send_dashboard(state, http, token, &chat_id, Some(msg_id)).await;
        return;
    }

    if data == "cb:status" {
        let _ = answer_callback(http, token, cb_id, Some("Обновление данных...")).await;
        let text = generate_status_report(state).await;
        let kb = InlineKeyboardMarkup {
            inline_keyboard: vec![
                vec![
                    InlineKeyboardButton { text: "🔄 Обновить".into(), callback_data: "cb:status".into() },
                    InlineKeyboardButton { text: "⚡ Пинг".into(), callback_data: "cb:ping".into() },
                ],
                vec![
                    InlineKeyboardButton { text: "◀️ В меню".into(), callback_data: "cb:menu".into() },
                ],
            ],
        };
        let _ = edit_msg(http, token, &chat_id, msg_id, &text, Some(json!(kb))).await;
        return;
    }

    if data == "cb:ping" {
        let _ = answer_callback(http, token, cb_id, Some("Замер пинга серверов...")).await;
        let text = generate_ping_report(state).await;
        let kb = InlineKeyboardMarkup {
            inline_keyboard: vec![
                vec![
                    InlineKeyboardButton { text: "⚡ Повторить".into(), callback_data: "cb:ping".into() },
                    InlineKeyboardButton { text: "🔄 Сменить сервер".into(), callback_data: "cb:servers".into() },
                ],
                vec![
                    InlineKeyboardButton { text: "◀️ В меню".into(), callback_data: "cb:menu".into() },
                ],
            ],
        };
        let _ = edit_msg(http, token, &chat_id, msg_id, &text, Some(json!(kb))).await;
        return;
    }

    if data == "cb:servers" {
        let _ = answer_callback(http, token, cb_id, None).await;
        send_servers_menu(state, http, token, &chat_id, Some(msg_id)).await;
        return;
    }

    if let Some(srv_name) = data.strip_prefix("srv:") {
        let cfg = state.config.read().await.clone();
        match crate::mihomo::switch_server(&state.http, &cfg, srv_name).await {
            Ok(_) => {
                let _ = answer_callback(http, token, cb_id, Some("✓ Сервер переключен")).await;
                send_servers_menu(state, http, token, &chat_id, Some(msg_id)).await;
            }
            Err(e) => {
                let _ = answer_callback(http, token, cb_id, Some(&format!("Ошибка: {}", e))).await;
            }
        }
        return;
    }

    if data == "cb:zapret" {
        let _ = answer_callback(http, token, cb_id, None).await;
        send_zapret_menu(state, http, token, &chat_id, Some(msg_id)).await;
        return;
    }

    if data == "cb:zapret_start" || data == "cb:zapret_stop" {
        let action = if data == "cb:zapret_start" { "start" } else { "stop" };
        let _ = answer_callback(http, token, cb_id, Some("Применение настроек Zapret...")).await;
        let res = zapret_toggle_action(action).await;
        if res.is_ok() {
            send_zapret_menu(state, http, token, &chat_id, Some(msg_id)).await;
        } else {
            let _ = send_msg(http, token, &chat_id, &format!("❌ Ошибка Zapret: {:?}", res.err()), None).await;
        }
        return;
    }

    if data == "cb:restart_confirm" {
        let _ = answer_callback(http, token, cb_id, Some("Службы перезапускаются...")).await;
        let _ = edit_msg(
            http,
            token,
            &chat_id,
            msg_id,
            "🔄 <b>Перезапуск служб XKeen и Mihomo запущен...</b>\nПодождите 5-10 секунд для завершения рестарта.",
            None,
        ).await;

        let restart_state = state.clone();
        tokio::spawn(async move {
            let cfg = restart_state.config.read().await.clone();
            let _ = crate::mihomo::reload_config(&restart_state.http, &cfg).await;
        });
        return;
    }

    if let Some(domain) = data.strip_prefix("add_force:") {
        let domain_str = domain.to_string();
        if let Ok(mut tx) = crate::transaction::ConfigTx::begin(state).await {
            if !tx.config().force_domains.iter().any(|d| d.eq_ignore_ascii_case(&domain_str)) {
                tx.config_mut().force_domains.push(domain_str.clone());
                let auto_cdns = crate::cdn_discovery::discover_all_cdns(&tx.config().force_domains, &tx.config().mihomo_proxy_url()).await;
                let mut all_force = tx.config().force_domains.clone();
                for cdn in &auto_cdns {
                    if !all_force.contains(cdn) {
                        all_force.push(cdn.clone());
                    }
                }
                if tx.config().zapret.enabled {
                    all_force.retain(|d| !crate::routing::is_zapret_direct_domain(d, &tx.config().zapret));
                }
                if let Ok(yaml) = tx.read_yaml().await {
                    if let Ok(mut new_yaml) = crate::routing::apply_domain_rules(&yaml, &tx.config().direct_domains, &all_force, &tx.config().device_domain_rules) {
                        if let Ok(with_zapret) = crate::routing::apply_zapret_hybrid_rules(&new_yaml, &tx.config().zapret) {
                            new_yaml = with_zapret;
                        }
                        let _ = tx.set_yaml(new_yaml);
                    }
                }
                let _ = tx.commit_and_reload().await;
                let _ = crate::override_sync::sync_geo_override(&all_force).await;

                let _ = answer_callback(http, token, cb_id, Some("✓ Домен добавлен в Force-Proxy")).await;
                let _ = send_msg(
                    http,
                    token,
                    &chat_id,
                    &format!("✅ Домен <code>{}</code> успешно добавлен в Force-Proxy и применён в ядре Mihomo!", domain_str),
                    None,
                ).await;
            } else {
                let _ = answer_callback(http, token, cb_id, Some("Домен уже в списке")).await;
            }
        }
        return;
    }

    let _ = answer_callback(http, token, cb_id, None).await;
}

/// Отправка или обновление главного дашборда
async fn send_dashboard(
    state: &AppState,
    http: &reqwest::Client,
    token: &str,
    chat_id: &str,
    edit_id: Option<i64>,
) {
    let cfg = state.config.read().await.clone();
    let rci_ver = crate::rci::get_version(&state.http, &cfg).await.ok();
    let model = rci_ver
        .as_ref()
        .and_then(|m| m.get("device").or_else(|| m.get("model")))
        .map(|s| s.as_str())
        .unwrap_or("Keenetic");

    let servers_res = crate::mihomo::get_servers(&state.http, &cfg, &cfg.failover.priority_chain).await;
    let active_server_name = servers_res
        .ok()
        .and_then(|list| list.into_iter().find(|s| s.is_active))
        .map(|s| s.name)
        .unwrap_or_else(|| "Не выбран".into());

    let zapret_running = is_zapret_running().await;

    let text = format!(
        "⚡ <b>Панель управления XKeen Route</b>\n\n\
        📟 <b>Роутер:</b> {}\n\
        🛡️ <b>Активный VPN:</b> <code>{}</code>\n\
        🚀 <b>DPI Zapret:</b> {}\n\
        ⚙️ <b>Версия панели:</b> <code>{}</code>\n\n\
        <i>Выберите действие кнопками ниже или отправьте домен для анализа маршрута:</i>",
        model,
        active_server_name,
        if zapret_running { "🟢 Активен" } else { "🔴 Выключен" },
        crate::VERSION,
    );

    let kb = InlineKeyboardMarkup {
        inline_keyboard: vec![
            vec![
                InlineKeyboardButton { text: "📊 Статус".into(), callback_data: "cb:status".into() },
                InlineKeyboardButton { text: "⚡ Пинг".into(), callback_data: "cb:ping".into() },
            ],
            vec![
                InlineKeyboardButton { text: "🔄 Сменить узел".into(), callback_data: "cb:servers".into() },
                InlineKeyboardButton { text: "🛡️ Zapret".into(), callback_data: "cb:zapret".into() },
            ],
            vec![
                InlineKeyboardButton { text: "🔁 Перезапуск XKeen".into(), callback_data: "cb:restart_confirm".into() },
            ],
        ],
    };

    if let Some(msg_id) = edit_id {
        let _ = edit_msg(http, token, chat_id, msg_id, &text, Some(json!(kb))).await;
    } else {
        let _ = send_msg(http, token, chat_id, &text, Some(json!(kb))).await;
    }
}

/// Генерация текстового отчета о статусе роутера
async fn generate_status_report(state: &AppState) -> String {
    let cfg = state.config.read().await.clone();
    let sys = crate::rci::get_system(&state.http, &cfg).await.ok();
    let rci_ver = crate::rci::get_version(&state.http, &cfg).await.ok();
    let model = rci_ver
        .as_ref()
        .and_then(|m| m.get("device").or_else(|| m.get("model")))
        .map(|s| s.as_str())
        .unwrap_or("Keenetic");

    let cpu_pct = sys.as_ref().map(|s| s.cpu_percent).unwrap_or(0);
    let cpu_temp = sys.as_ref().and_then(|s| s.cpu_temp_c).unwrap_or(0.0);
    let mem_used = sys.as_ref().map(|s| s.memory_used_mb).unwrap_or(0);
    let mem_total = sys.as_ref().map(|s| s.memory_total_mb).unwrap_or(0);
    let core_mem = sys.as_ref().map(|s| s.core_memory_mb).unwrap_or(0.0);

    let servers = crate::mihomo::get_servers(&state.http, &cfg, &cfg.failover.priority_chain).await.ok().unwrap_or_default();
    let (act_name, ping_ms) = servers.into_iter()
        .find(|s| s.is_active)
        .map(|s| (s.name, s.ping_ms))
        .unwrap_or(("Не выбран".into(), 0));

    let zapret_running = is_zapret_running().await;

    format!(
        "📊 <b>Системный статус роутера</b>\n\n\
        📟 <b>Модель:</b> {}\n\
        🌡️ <b>Температура CPU:</b> {:.1}°C\n\
        ⚡ <b>Загрузка CPU:</b> {}%\n\
        🧠 <b>ОЗУ:</b> {} MB / {} MB\n\
        💾 <b>Mihomo Core:</b> {:.1} MB\n\n\
        🌐 <b>Активный узел:</b> <code>{}</code>\n\
        ⏱️ <b>Задержка (пинг):</b> {} мс\n\
        🛡️ <b>Zapret DPI:</b> {}\n\
        🔄 <b>Failover:</b> {}",
        model,
        cpu_temp,
        cpu_pct,
        mem_used,
        mem_total,
        core_mem,
        act_name,
        if ping_ms > 0 { format!("{ping_ms}") } else { "—".into() },
        if zapret_running { "🟢 Работает" } else { "🔴 Остановлен" },
        if cfg.failover.enabled { "🟢 Включен" } else { "⚪ Отключен" },
    )
}

/// Меню выбора серверов
async fn send_servers_menu(
    state: &AppState,
    http: &reqwest::Client,
    token: &str,
    chat_id: &str,
    edit_id: Option<i64>,
) {
    let cfg = state.config.read().await.clone();
    let servers_res = crate::mihomo::get_servers(&state.http, &cfg, &cfg.failover.priority_chain).await;
    let all_servers = match servers_res {
        Ok(res) => res,
        Err(e) => {
            let err_text = format!("❌ Не удалось получить список серверов: {e}");
            if let Some(mid) = edit_id {
                let _ = edit_msg(http, token, chat_id, mid, &err_text, None).await;
            } else {
                let _ = send_msg(http, token, chat_id, &err_text, None).await;
            }
            return;
        }
    };

    let act_name = all_servers.iter().find(|s| s.is_active).map(|s| s.name.clone()).unwrap_or_default();
    let mut keyboard = Vec::new();

    // Показываем первые 8 серверов для удобства мобильного интерфейса
    for s in all_servers.iter().filter(|s| !s.is_pool).take(8) {
        let is_current = s.is_active;
        let prefix = if is_current { "★ " } else { "" };
        let btn_text = format!("{prefix}{}", s.name);
        keyboard.push(vec![InlineKeyboardButton {
            text: btn_text,
            callback_data: format!("srv:{}", s.id),
        }]);
    }

    keyboard.push(vec![
        InlineKeyboardButton { text: "◀️ В меню".into(), callback_data: "cb:menu".into() },
    ]);

    let text = format!(
        "🔄 <b>Выбор активного VPN-узла</b>\n\n\
        Текущий сервер: <code>{}</code>\n\
        <i>Нажмите на кнопку с сервером для мгновенного переключения:</i>",
        if act_name.is_empty() { "Не выбран" } else { &act_name }
    );

    let kb = InlineKeyboardMarkup { inline_keyboard: keyboard };
    if let Some(mid) = edit_id {
        let _ = edit_msg(http, token, chat_id, mid, &text, Some(json!(kb))).await;
    } else {
        let _ = send_msg(http, token, chat_id, &text, Some(json!(kb))).await;
    }
}

/// Замер пинга серверов
async fn generate_ping_report(state: &AppState) -> String {
    let cfg = state.config.read().await.clone();
    let servers = crate::mihomo::get_servers(&state.http, &cfg, &cfg.failover.priority_chain).await.ok().unwrap_or_default();

    if servers.is_empty() {
        return "❌ Серверы не найдены в конфигурации Mihomo.".into();
    }

    let mut report = String::from("⚡ <b>Замер задержки (Ping) серверов:</b>\n\n");
    let test_ids: Vec<String> = servers.iter().filter(|s| !s.is_pool).take(6).map(|s| s.id.clone()).collect();
    let ping_map = crate::mihomo::ping_all(&state.http, &cfg, &test_ids, 2500).await;

    for s in servers.iter().filter(|s| !s.is_pool).take(6) {
        let p = ping_map.get(&s.id).copied().unwrap_or(0);
        let status_emoji = if p == 0 {
            "🔴"
        } else if p < 80 {
            "🟢"
        } else if p < 150 {
            "🟡"
        } else {
            "🟠"
        };
        let p_str = if p > 0 { format!("{p} мс") } else { "Таймаут".to_string() };
        report.push_str(&format!("{status_emoji} <b>{}</b>: <code>{}</code>\n", s.name, p_str));
    }

    report
}

/// Меню управления Zapret DPI
async fn send_zapret_menu(
    _state: &AppState,
    http: &reqwest::Client,
    token: &str,
    chat_id: &str,
    edit_id: Option<i64>,
) {
    let running = is_zapret_running().await;

    let text = format!(
        "🛡️ <b>Управление Zapret DPI (Обход замедлений)</b>\n\n\
        • Статус: {}\n\n\
        <i>Zapret десинхронизирует пакеты ТСПУ на уровне роутера для прямого воспроизведения YouTube и Discord без VPN.</i>",
        if running { "🟢 <b>Активен (Служба работает)</b>" } else { "🔴 <b>Остановлен</b>" },
    );

    let kb = InlineKeyboardMarkup {
        inline_keyboard: vec![
            vec![
                if running {
                    InlineKeyboardButton { text: "🔴 Остановить Zapret".into(), callback_data: "cb:zapret_stop".into() }
                } else {
                    InlineKeyboardButton { text: "🟢 Запустить Zapret".into(), callback_data: "cb:zapret_start".into() }
                },
            ],
            vec![
                InlineKeyboardButton { text: "◀️ В главное меню".into(), callback_data: "cb:menu".into() },
            ],
        ],
    };

    if let Some(mid) = edit_id {
        let _ = edit_msg(http, token, chat_id, mid, &text, Some(json!(kb))).await;
    } else {
        let _ = send_msg(http, token, chat_id, &text, Some(json!(kb))).await;
    }
}

/// Анализ маршрута введенного пользователем домена
async fn handle_domain_analysis(
    state: &AppState,
    http: &reqwest::Client,
    token: &str,
    chat_id: &str,
    domain: &str,
) {
    let cfg = state.config.read().await.clone();
    let is_force = cfg.force_domains.iter().any(|d| d.eq_ignore_ascii_case(domain));

    let text = format!(
        "🌐 <b>Анализ маршрутизации домена:</b> <code>{}</code>\n\n\
        • Принудительный Force-Proxy: {}\n\
        • Назначение: {}\n",
        domain,
        if is_force { "✓ <b>В списке принудительного обхода</b>" } else { "— Обычный маршрут" },
        if is_force { "🛡️ Трафик всегда идёт через активный VPN-узел" } else { "🌐 Определяется стандартными правилами Mihomo" }
    );

    let mut buttons = Vec::new();
    if !is_force {
        buttons.push(vec![InlineKeyboardButton {
            text: format!("➕ Добавить {} в Force-Proxy", domain),
            callback_data: format!("add_force:{}", domain),
        }]);
    }
    buttons.push(vec![InlineKeyboardButton {
        text: "◀️ В меню".into(),
        callback_data: "cb:menu".into(),
    }]);

    let kb = InlineKeyboardMarkup { inline_keyboard: buttons };
    let _ = send_msg(http, token, chat_id, &text, Some(json!(kb))).await;
}

// ----------------- Базовые вызовы Telegram Bot API -----------------

async fn send_msg(
    http: &reqwest::Client,
    token: &str,
    chat_id: &str,
    text: &str,
    reply_markup: Option<Value>,
) -> Result<(), String> {
    let url = format!("https://api.telegram.org/bot{}/sendMessage", token);
    let mut payload = json!({
        "chat_id": chat_id,
        "text": text,
        "parse_mode": "HTML",
        "disable_web_page_preview": true
    });
    if let Some(rm) = reply_markup {
        payload["reply_markup"] = rm;
    }

    let res = http
        .post(&url)
        .header("User-Agent", "xkeen-route")
        .json(&payload)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if res.status().is_success() {
        Ok(())
    } else {
        Err(format!("HTTP {}", res.status()))
    }
}

async fn edit_msg(
    http: &reqwest::Client,
    token: &str,
    chat_id: &str,
    message_id: i64,
    text: &str,
    reply_markup: Option<Value>,
) -> Result<(), String> {
    let url = format!("https://api.telegram.org/bot{}/editMessageText", token);
    let mut payload = json!({
        "chat_id": chat_id,
        "message_id": message_id,
        "text": text,
        "parse_mode": "HTML",
        "disable_web_page_preview": true
    });
    if let Some(rm) = reply_markup {
        payload["reply_markup"] = rm;
    }

    let res = http
        .post(&url)
        .header("User-Agent", "xkeen-route")
        .json(&payload)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if res.status().is_success() {
        Ok(())
    } else {
        Err(format!("HTTP {}", res.status()))
    }
}

async fn answer_callback(
    http: &reqwest::Client,
    token: &str,
    cb_id: &str,
    text: Option<&str>,
) -> Result<(), String> {
    let url = format!("https://api.telegram.org/bot{}/answerCallbackQuery", token);
    let mut payload = json!({ "callback_query_id": cb_id });
    if let Some(t) = text {
        payload["text"] = json!(t);
    }

    let _ = http
        .post(&url)
        .header("User-Agent", "xkeen-route")
        .json(&payload)
        .send()
        .await;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_authorized() {
        assert!(is_authorized("123456789", "123456789"));
        assert!(is_authorized(" 123456789 ", "123456789"));
        assert!(!is_authorized("999999999", "123456789"));
        assert!(!is_authorized("", "123456789"));
        assert!(!is_authorized("123456789", ""));
    }

    #[test]
    fn test_inline_keyboard_serialization() {
        let kb = InlineKeyboardMarkup {
            inline_keyboard: vec![vec![InlineKeyboardButton {
                text: "Тест".into(),
                callback_data: "cb:test".into(),
            }]],
        };
        let val = serde_json::to_value(&kb).unwrap();
        assert!(val.get("inline_keyboard").is_some());
        let first_btn = &val["inline_keyboard"][0][0];
        assert_eq!(first_btn["text"], "Тест");
        assert_eq!(first_btn["callback_data"], "cb:test");
    }

    #[test]
    fn test_telegram_update_deserialization() {
        // Тест парсинга входящего текстового сообщения
        let msg_json = r#"{
            "update_id": 100,
            "message": {
                "message_id": 42,
                "chat": { "id": 123456789 },
                "text": "instagram.com"
            }
        }"#;
        let update: TelegramUpdate = serde_json::from_str(msg_json).expect("valid message update");
        assert_eq!(update.update_id, 100);
        let msg = update.message.expect("contains message");
        assert_eq!(msg.message_id, 42);
        assert_eq!(msg.chat.id, 123456789);
        assert_eq!(msg.text.as_deref(), Some("instagram.com"));

        // Тест парсинга callback query (нажатие кнопки)
        let cb_json = r#"{
            "update_id": 101,
            "callback_query": {
                "id": "cb_query_999",
                "message": {
                    "message_id": 43,
                    "chat": { "id": 123456789 },
                    "text": "Dashboard"
                },
                "data": "cb:status"
            }
        }"#;
        let update_cb: TelegramUpdate = serde_json::from_str(cb_json).expect("valid callback update");
        assert_eq!(update_cb.update_id, 101);
        let cb = update_cb.callback_query.expect("contains callback");
        assert_eq!(cb.id, "cb_query_999");
        assert_eq!(cb.data.as_deref(), Some("cb:status"));
    }

    #[test]
    fn test_bot_domain_analysis_normalization() {
        let input = "  https://www.rutracker.org/forum/index.php  ";
        let clean = crate::config::normalize_domain(input);
        assert_eq!(clean, "rutracker.org");
        assert!(crate::config::is_valid_domain(&clean));

        let invalid = "not a domain @@";
        let clean_inv = crate::config::normalize_domain(invalid);
        assert!(!crate::config::is_valid_domain(&clean_inv));
    }
}
