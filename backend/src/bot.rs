//! Telegram Bot Daemon для управления роутером Keenetic через XKeen Route.
//! Поддерживает Long Polling через локальный прокси Mihomo (обход блокировок ТСПУ),
//! авторизацию по Chat ID, интерактивные инлайн-клавиатуры и полный спектр оперативных команд:
//! статус, устройства в сети, тест скорости (Speedtest), выбор узлов, пинг, Zapret-пресеты,
//! блокировка рекламы (AdBlock), кнопка экстренной паники (100% Direct), резервное копирование
//! config.json в виде файла-документа, журнал Failover и обновление панели в 1 клик.

use std::time::Duration;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::time::sleep;

use crate::{log_i, log_w, AppState};

/// Хранение предыдущего активного сервера перед активацией аварийного режима (Panic Button)
static PRE_EMERGENCY_SERVER: tokio::sync::Mutex<Option<String>> = tokio::sync::Mutex::const_new(None);

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
                .timeout(Duration::from_secs(45))
                .build()
            {
                return Some(c);
            }
        }
    }
    reqwest::Client::builder()
        .timeout(Duration::from_secs(45))
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
                log_i!("[BOT] Расширенное меню команд успешно зарегистрировано в Telegram Bot API");
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
            { "command": "start", "description": "📱 Главный пульт управления" },
            { "command": "status", "description": "📊 Статус системы, CPU, RAM и узлов" },
            { "command": "devices", "description": "👥 Устройства в локальной сети" },
            { "command": "speedtest", "description": "🚀 Замер скорости интернет-канала" },
            { "command": "servers", "description": "🔄 Выбор активного VPN-узла" },
            { "command": "ping", "description": "⚡ Замер пинга до серверов" },
            { "command": "zapret", "description": "🛡️ Управление и пресеты DPI Zapret" },
            { "command": "adblock", "description": "🛑 Блокировка рекламы" },
            { "command": "emergency", "description": "🚨 Режим паники (100% Direct)" },
            { "command": "history", "description": "📜 Журнал переключений Failover" },
            { "command": "backup", "description": "💾 Резервная копия config.json" },
            { "command": "update", "description": "⚡ Проверка и установка обновлений" },
            { "command": "restart", "description": "🔁 Перезапуск служб XKeen" },
            { "command": "help", "description": "ℹ️ Справка и возможности бота" }
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
        "/devices" => {
            let dev_text = generate_devices_report(state).await;
            let kb = InlineKeyboardMarkup {
                inline_keyboard: vec![
                    vec![
                        InlineKeyboardButton { text: "🔄 Обновить".into(), callback_data: "cb:devices".into() },
                        InlineKeyboardButton { text: "◀️ В главное меню".into(), callback_data: "cb:menu".into() },
                    ],
                ],
            };
            let _ = send_msg(http, token, chat_id, &dev_text, Some(json!(kb))).await;
        }
        "/speedtest" => {
            let _ = send_msg(http, token, chat_id, "⏳ <i>Запуск замера скорости (Speedtest)... Пожалуйста, подождите 5-10 сек.</i>", None).await;
            let speed_text = generate_speedtest_report(state).await;
            let kb = InlineKeyboardMarkup {
                inline_keyboard: vec![
                    vec![
                        InlineKeyboardButton { text: "🚀 Повторить".into(), callback_data: "cb:speedtest".into() },
                        InlineKeyboardButton { text: "◀️ В меню".into(), callback_data: "cb:menu".into() },
                    ],
                ],
            };
            let _ = send_msg(http, token, chat_id, &speed_text, Some(json!(kb))).await;
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
        "/adblock" => {
            match bot_toggle_adblock(state).await {
                Ok(new_val) => {
                    let msg = if new_val {
                        "🛑 <b>Блокировка рекламы (AdBlock) включена</b>\nФильтрация трекеров и рекламных баннеров активирована."
                    } else {
                        "⚪ <b>Блокировка рекламы (AdBlock) отключена</b>\nЗапросы передаются без фильтрации."
                    };
                    let kb = InlineKeyboardMarkup {
                        inline_keyboard: vec![
                            vec![
                                InlineKeyboardButton { text: "🔄 Переключить снова".into(), callback_data: "cb:adblock_toggle".into() },
                                InlineKeyboardButton { text: "◀️ В меню".into(), callback_data: "cb:menu".into() },
                            ],
                        ],
                    };
                    let _ = send_msg(http, token, chat_id, msg, Some(json!(kb))).await;
                }
                Err(e) => {
                    let _ = send_msg(http, token, chat_id, &format!("❌ Ошибка переключения AdBlock: {e}"), None).await;
                }
            }
        }
        "/emergency" => {
            handle_emergency_toggle(state, http, token, chat_id, None).await;
        }
        "/history" => {
            let hist_text = generate_history_report(state).await;
            let kb = InlineKeyboardMarkup {
                inline_keyboard: vec![
                    vec![
                        InlineKeyboardButton { text: "🔄 Обновить".into(), callback_data: "cb:history".into() },
                        InlineKeyboardButton { text: "◀️ В меню".into(), callback_data: "cb:menu".into() },
                    ],
                ],
            };
            let _ = send_msg(http, token, chat_id, &hist_text, Some(json!(kb))).await;
        }
        "/backup" => {
            handle_backup_request(state, http, token, chat_id).await;
        }
        "/update" => {
            send_update_menu(state, http, token, chat_id, None).await;
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

    if data == "cb:devices" {
        let _ = answer_callback(http, token, cb_id, Some("Опрос устройств Keenetic RCI...")).await;
        let text = generate_devices_report(state).await;
        let kb = InlineKeyboardMarkup {
            inline_keyboard: vec![
                vec![
                    InlineKeyboardButton { text: "🔄 Обновить".into(), callback_data: "cb:devices".into() },
                    InlineKeyboardButton { text: "◀️ В меню".into(), callback_data: "cb:menu".into() },
                ],
            ],
        };
        let _ = edit_msg(http, token, &chat_id, msg_id, &text, Some(json!(kb))).await;
        return;
    }

    if data == "cb:speedtest" {
        let _ = answer_callback(http, token, cb_id, Some("Запуск теста скорости...")).await;
        let _ = edit_msg(
            http,
            token,
            &chat_id,
            msg_id,
            "🚀 <b>Выполняется замер скорости через активный узел...</b>\nПожалуйста, подождите 5-10 секунд.",
            None,
        ).await;
        let text = generate_speedtest_report(state).await;
        let kb = InlineKeyboardMarkup {
            inline_keyboard: vec![
                vec![
                    InlineKeyboardButton { text: "🚀 Повторить".into(), callback_data: "cb:speedtest".into() },
                    InlineKeyboardButton { text: "◀️ В меню".into(), callback_data: "cb:menu".into() },
                ],
            ],
        };
        let _ = edit_msg(http, token, &chat_id, msg_id, &text, Some(json!(kb))).await;
        return;
    }

    if data == "cb:ping" {
        let _ = answer_callback(http, token, cb_id, Some("Замер пинга узлов...")).await;
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
        let enable = data == "cb:zapret_start";
        let _ = answer_callback(http, token, cb_id, Some("Применение настроек Zapret...")).await;
        // X-04: полное ядро переключения (конфиг + маркер автозапуска + S51zapret),
        // а не прямой вызов скрипта — иначе watchdog откатывает действие бота.
        let res = crate::api::zapret_set_enabled_core(state, enable).await;
        match res {
            Ok(msg) => {
                let _ = answer_callback(http, token, cb_id, Some(&msg)).await;
                send_zapret_menu(state, http, token, &chat_id, Some(msg_id)).await;
            }
            Err(e) => {
                let _ = send_msg(http, token, &chat_id, &format!("❌ Ошибка Zapret: {e}"), None).await;
            }
        }
        return;
    }

    if let Some(preset_name) = data.strip_prefix("cb:preset:") {
        let _ = answer_callback(http, token, cb_id, Some("Применение пресета DPI...")).await;
        match crate::api::apply_zapret_preset_action(state, preset_name, None).await {
            Ok(msg) => {
                let _ = send_msg(
                    http,
                    token,
                    &chat_id,
                    &format!("✅ <b>Пресет применён:</b> <code>{}</code>\n{}", preset_name, msg),
                    None,
                ).await;
                send_zapret_menu(state, http, token, &chat_id, Some(msg_id)).await;
            }
            Err(e) => {
                let _ = send_msg(http, token, &chat_id, &format!("❌ Ошибка применения пресета: {e}"), None).await;
            }
        }
        return;
    }

    if data == "cb:adblock_toggle" {
        let _ = answer_callback(http, token, cb_id, Some("Переключение AdBlock...")).await;
        match bot_toggle_adblock(state).await {
            Ok(new_val) => {
                let _ = answer_callback(http, token, cb_id, Some(if new_val { "AdBlock включен" } else { "AdBlock выключен" })).await;
                send_dashboard(state, http, token, &chat_id, Some(msg_id)).await;
            }
            Err(e) => {
                let _ = send_msg(http, token, &chat_id, &format!("❌ Ошибка AdBlock: {e}"), None).await;
            }
        }
        return;
    }

    if data == "cb:emergency" {
        let _ = answer_callback(http, token, cb_id, Some("🚨 Включение режима паники...")).await;
        handle_emergency_toggle(state, http, token, &chat_id, Some(msg_id)).await;
        return;
    }

    if data == "cb:emergency_resume" {
        let _ = answer_callback(http, token, cb_id, Some("Возобновление обхода...")).await;
        handle_emergency_resume(state, http, token, &chat_id, Some(msg_id)).await;
        return;
    }

    if data == "cb:history" {
        let _ = answer_callback(http, token, cb_id, None).await;
        let text = generate_history_report(state).await;
        let kb = InlineKeyboardMarkup {
            inline_keyboard: vec![
                vec![
                    InlineKeyboardButton { text: "🔄 Обновить".into(), callback_data: "cb:history".into() },
                    InlineKeyboardButton { text: "◀️ В меню".into(), callback_data: "cb:menu".into() },
                ],
            ],
        };
        let _ = edit_msg(http, token, &chat_id, msg_id, &text, Some(json!(kb))).await;
        return;
    }

    if data == "cb:backup" {
        let _ = answer_callback(http, token, cb_id, Some("Формирование бэкапа...")).await;
        handle_backup_request(state, http, token, &chat_id).await;
        return;
    }

    if data == "cb:update_check" {
        let _ = answer_callback(http, token, cb_id, Some("Проверка GitHub...")).await;
        send_update_menu(state, http, token, &chat_id, Some(msg_id)).await;
        return;
    }

    if data == "cb:update_install" {
        let _ = answer_callback(http, token, cb_id, Some("⚡ Запуск установки обновления...")).await;
        let _ = edit_msg(
            http,
            token,
            &chat_id,
            msg_id,
            "⏳ <b>Загрузка и установка обновления начата!</b>\nБинарник скачивается и проверяется. Роутер автоматически перезапустит веб-панель через 10-15 секунд.",
            None,
        ).await;

        let update_state = state.clone();
        tokio::spawn(async move {
            let _ = crate::updater::install(axum::extract::State(update_state)).await;
        });
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
        let _ = answer_callback(http, token, cb_id, Some("Добавление в Force-Proxy...")).await;
        match bot_add_force_domain(state, &domain_str).await {
            Ok(_) => {
                let _ = edit_msg(
                    http,
                    token,
                    &chat_id,
                    msg_id,
                    &format!("✅ Домен <code>{}</code> успешно добавлен в <b>Force-Proxy</b> и направлен в тоннель!", domain_str),
                    Some(json!({
                        "inline_keyboard": [[InlineKeyboardButton { text: "◀️ В меню".into(), callback_data: "cb:menu".into() }]]
                    })),
                ).await;
            }
            Err(e) => {
                let _ = send_msg(http, token, &chat_id, &format!("❌ Ошибка добавления домена: {e}"), None).await;
            }
        }
        return;
    }

    if let Some(domain) = data.strip_prefix("add_direct:") {
        let domain_str = domain.to_string();
        let _ = answer_callback(http, token, cb_id, Some("Добавление в Direct...")).await;
        match bot_add_direct_domain(state, &domain_str).await {
            Ok(_) => {
                let _ = edit_msg(
                    http,
                    token,
                    &chat_id,
                    msg_id,
                    &format!("✅ Домен <code>{}</code> успешно добавлен в <b>Direct</b> (напрямую без VPN)!", domain_str),
                    Some(json!({
                        "inline_keyboard": [[InlineKeyboardButton { text: "◀️ В меню".into(), callback_data: "cb:menu".into() }]]
                    })),
                ).await;
            }
            Err(e) => {
                let _ = send_msg(http, token, &chat_id, &format!("❌ Ошибка добавления в Direct: {e}"), None).await;
            }
        }
        return;
    }

    if let Some(domain) = data.strip_prefix("del_rule:") {
        let domain_str = domain.to_string();
        let _ = answer_callback(http, token, cb_id, Some("Удаление из правил...")).await;
        match bot_remove_domain_rules(state, &domain_str).await {
            Ok(_) => {
                let _ = edit_msg(
                    http,
                    token,
                    &chat_id,
                    msg_id,
                    &format!("🗑️ Домен <code>{}</code> удалён из пользовательских правил.", domain_str),
                    Some(json!({
                        "inline_keyboard": [[InlineKeyboardButton { text: "◀️ В меню".into(), callback_data: "cb:menu".into() }]]
                    })),
                ).await;
            }
            Err(e) => {
                let _ = send_msg(http, token, &chat_id, &format!("❌ Ошибка удаления домена: {e}"), None).await;
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
        🛑 <b>AdBlock:</b> {}\n\
        ⚙️ <b>Версия:</b> <code>{}</code>\n\n\
        <i>Выберите действие кнопками ниже или отправьте домен для анализа маршрута:</i>",
        model,
        active_server_name,
        if zapret_running { "🟢 Активен" } else { "🔴 Выключен" },
        if cfg.adblock_enabled { "🟢 Включен" } else { "⚪ Отключен" },
        crate::VERSION,
    );

    let kb = InlineKeyboardMarkup {
        inline_keyboard: vec![
            vec![
                InlineKeyboardButton { text: "📊 Статус".into(), callback_data: "cb:status".into() },
                InlineKeyboardButton { text: "⚡ Пинг".into(), callback_data: "cb:ping".into() },
            ],
            vec![
                InlineKeyboardButton { text: "👥 Устройства".into(), callback_data: "cb:devices".into() },
                InlineKeyboardButton { text: "🚀 Скорость".into(), callback_data: "cb:speedtest".into() },
            ],
            vec![
                InlineKeyboardButton { text: "🔄 Сменить узел".into(), callback_data: "cb:servers".into() },
                InlineKeyboardButton { text: "🛡️ Zapret DPI".into(), callback_data: "cb:zapret".into() },
            ],
            vec![
                InlineKeyboardButton {
                    text: if cfg.adblock_enabled { "🛑 AdBlock: Вкл" } else { "⚪ AdBlock: Выкл" }.into(),
                    callback_data: "cb:adblock_toggle".into(),
                },
                InlineKeyboardButton { text: "🚨 ПАНИКА".into(), callback_data: "cb:emergency".into() },
            ],
            vec![
                InlineKeyboardButton { text: "📜 Failover".into(), callback_data: "cb:history".into() },
                InlineKeyboardButton { text: "💾 Бэкап config".into(), callback_data: "cb:backup".into() },
            ],
            vec![
                InlineKeyboardButton { text: "⚡ Обновление".into(), callback_data: "cb:update_check".into() },
                InlineKeyboardButton { text: "🔁 Перезапуск".into(), callback_data: "cb:restart_confirm".into() },
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
        🛑 <b>AdBlock:</b> {}\n\
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
        if cfg.adblock_enabled { "🟢 Включен" } else { "⚪ Отключен" },
        if cfg.failover.enabled { "🟢 Включен" } else { "⚪ Отключен" },
    )
}

/// Генерация списка устройств в локальной сети Keenetic
async fn generate_devices_report(state: &AppState) -> String {
    let cfg = state.config.read().await.clone();
    let devices = match crate::rci::get_devices(&state.http, &cfg, &Vec::new(), "").await {
        Ok(d) => d,
        Err(e) => return format!("❌ Ошибка получения списка устройств Keenetic RCI: {e}"),
    };

    format_devices_report_str(&devices)
}

pub fn format_devices_report_str(devices: &[crate::rci::Device]) -> String {
    if devices.is_empty() {
        return "👥 <b>Устройства в сети:</b>\n\nВ локальной сети не обнаружено активных устройств.".into();
    }

    let online_count = devices.iter().filter(|d| d.online).count();
    let mut out = format!("👥 <b>Устройства в сети Keenetic</b> (Всего: {}, Онлайн: 🟢 {})\n\n", devices.len(), online_count);

    let mut sorted = devices.to_vec();
    sorted.sort_by_key(|d| !d.online);

    for d in sorted.into_iter().take(12) {
        let icon = if d.online { "🟢" } else { "⚪" };
        let ip_str = if d.ip.is_empty() { "—" } else { &d.ip };
        out.push_str(&format!(
            "{} <b>{}</b>\n   └ IP: <code>{}</code> | MAC: <code>{}</code>\n",
            icon, d.name, ip_str, d.mac
        ));
    }

    out
}

/// Замер скорости интернета (Speedtest) через активный сервер
async fn generate_speedtest_report(state: &AppState) -> String {
    let cfg = state.config.read().await.clone();
    let servers = crate::mihomo::get_servers(&state.http, &cfg, &cfg.failover.priority_chain).await.ok().unwrap_or_default();
    let active_server = servers.into_iter().find(|s| s.is_active);
    let srv = match active_server {
        Some(s) => s,
        None => return "❌ Нет активного сервера для выполнения Speedtest.".into(),
    };

    match crate::speedtest::run_speedtest(&state.http, &cfg, &srv.id).await {
        Ok(res) => format_speedtest_report_str(&srv.name, &res),
        Err(e) => format!("❌ Ошибка замера скорости: {e}"),
    }
}

pub fn format_speedtest_report_str(server_name: &str, res: &crate::speedtest::SpeedtestResponse) -> String {
    let mb_downloaded = (res.bytes_downloaded as f64) / (1024.0 * 1024.0);
    format!(
        "🚀 <b>Результаты замера скорости (Speedtest)</b>\n\n\
        🌐 <b>Сервер:</b> <code>{}</code>\n\
        ⚡ <b>Скорость загрузки:</b> <b>{:.1} Мбит/с</b>\n\
        ⏱️ <b>Задержка (Ping):</b> {} мс\n\
        📦 <b>Скачано:</b> {:.1} МБ за {:.1} сек",
        server_name,
        res.speed_mbps,
        res.latency_ms,
        mb_downloaded,
        res.duration_secs,
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

/// Меню управления Zapret DPI с кнопками пресетов
async fn send_zapret_menu(
    state: &AppState,
    http: &reqwest::Client,
    token: &str,
    chat_id: &str,
    edit_id: Option<i64>,
) {
    let running = is_zapret_running().await;
    let cfg = state.config.read().await.clone();

    let text = format!(
        "🛡️ <b>Управление Zapret DPI (Обход замедлений)</b>\n\n\
        • Статус: {}\n\
        • YouTube Turbo: {}\n\
        • Discord Voice UDP: {}\n\
        • Агрессивный DPI: {}\n\n\
        <i>Zapret десинхронизирует пакеты ТСПУ на роутере. Выберите быстрый пресет:</i>",
        if running { "🟢 <b>Активен (Служба работает)</b>" } else { "🔴 <b>Остановлен</b>" },
        if cfg.zapret.youtube_turbo { "🟢 Вкл" } else { "⚪ Выкл" },
        if cfg.zapret.discord_voice_udp { "🟢 Вкл" } else { "⚪ Выкл" },
        if cfg.zapret.aggressive_dpi { "🟢 Вкл" } else { "⚪ Выкл" },
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
                InlineKeyboardButton { text: "🎬 YouTube Turbo".into(), callback_data: "cb:preset:youtube".into() },
                InlineKeyboardButton { text: "🎧 Discord Voice".into(), callback_data: "cb:preset:discord".into() },
            ],
            vec![
                InlineKeyboardButton { text: "🎮 Игровой режим".into(), callback_data: "cb:preset:gamer".into() },
                InlineKeyboardButton { text: "⚡ Агрессивный DPI".into(), callback_data: "cb:preset:aggressive".into() },
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

/// Экстренный режим паники (Panic Button): переключение на 100% Direct
async fn handle_emergency_toggle(
    state: &AppState,
    http: &reqwest::Client,
    token: &str,
    chat_id: &str,
    edit_id: Option<i64>,
) {
    let cfg = state.config.read().await.clone();
    let servers = crate::mihomo::get_servers(&state.http, &cfg, &cfg.failover.priority_chain).await.ok().unwrap_or_default();
    let active_node = servers.into_iter().find(|s| s.is_active).map(|s| s.name).unwrap_or_else(|| "Fastest".to_string());

    *PRE_EMERGENCY_SERVER.lock().await = Some(active_node);
    let _ = crate::mihomo::switch_server(&state.http, &cfg, "DIRECT").await;
    let _ = crate::mihomo::close_all_connections(&state.http, &cfg).await;

    let text = "🚨 <b>РЕЖИМ ПАНИКИ АКТИВИРОВАН!</b>\n\n\
        ⚠️ Все прокси-тоннели временно отключены.\n\
        Весь трафик роутера идёт <b>напрямую через провайдера (100% DIRECT)</b>.\n\
        Ваши настройки, подписки и списки правил сохранены.\n\n\
        <i>Нажмите кнопку ниже для немедленного возобновления обхода:</i>";

    let kb = InlineKeyboardMarkup {
        inline_keyboard: vec![
            vec![
                InlineKeyboardButton { text: "▶️ Возобновить обход блокировок".into(), callback_data: "cb:emergency_resume".into() },
            ],
            vec![
                InlineKeyboardButton { text: "◀️ В меню".into(), callback_data: "cb:menu".into() },
            ],
        ],
    };

    if let Some(mid) = edit_id {
        let _ = edit_msg(http, token, chat_id, mid, text, Some(json!(kb))).await;
    } else {
        let _ = send_msg(http, token, chat_id, text, Some(json!(kb))).await;
    }
}

/// Возобновление обхода блокировок после режима паники
async fn handle_emergency_resume(
    state: &AppState,
    http: &reqwest::Client,
    token: &str,
    chat_id: &str,
    edit_id: Option<i64>,
) {
    let cfg = state.config.read().await.clone();
    let target = PRE_EMERGENCY_SERVER.lock().await.take().unwrap_or_else(|| "Fastest".to_string());
    let _ = crate::mihomo::switch_server(&state.http, &cfg, &target).await;
    let _ = crate::mihomo::close_all_connections(&state.http, &cfg).await;

    let text = format!(
        "✅ <b>Обход блокировок возобновлен!</b>\n\n\
        Активный маршрут переключен на: <code>{}</code>\n\
        Все соединения снова защищены и проксируются согласно правилам.",
        target
    );

    let kb = InlineKeyboardMarkup {
        inline_keyboard: vec![
            vec![
                InlineKeyboardButton { text: "📊 Статус".into(), callback_data: "cb:status".into() },
                InlineKeyboardButton { text: "◀️ В меню".into(), callback_data: "cb:menu".into() },
            ],
        ],
    };

    if let Some(mid) = edit_id {
        let _ = edit_msg(http, token, chat_id, mid, &text, Some(json!(kb))).await;
    } else {
        let _ = send_msg(http, token, chat_id, &text, Some(json!(kb))).await;
    }
}

/// Журнал событий Failover
async fn generate_history_report(state: &AppState) -> String {
    let events = state.failover_log.snapshot().await;
    format_history_report_str(&events)
}

pub fn format_history_report_str(events: &[crate::failover::FailoverEvent]) -> String {
    if events.is_empty() {
        return "📜 <b>Журнал событий Failover:</b>\n\nСобытий пока нет. Мониторинг доступности узлов активен.".into();
    }

    let mut out = String::from("📜 <b>Журнал переключений Failover (последние события):</b>\n\n");
    for ev in events.iter().take(8) {
        let icon = if ev.switched { "⚠️" } else { "ℹ️" };
        out.push_str(&format!("{} [<code>{}</code>] {}\n", icon, ev.time, ev.message));
    }
    out
}

/// Меню обновлений панели в 1 клик
async fn send_update_menu(
    state: &AppState,
    http: &reqwest::Client,
    token: &str,
    chat_id: &str,
    edit_id: Option<i64>,
) {
    let (current, latest, update_available, notes) = crate::updater::check_update_info(state).await;

    let text = if update_available {
        let mut notes_text = String::new();
        for n in notes.into_iter().take(5) {
            notes_text.push_str(&format!("• {}\n", n));
        }
        format!(
            "⚡ <b>Доступно обновление XKeen Route!</b> 🚀\n\n\
            📟 Текущая версия: <code>{}</code>\n\
            🆕 Новая версия: <code>{}</code>\n\n\
            <b>Что нового:</b>\n{}\n\
            <i>Нажмите кнопку ниже для автоматического обновления роутера в 1 клик:</i>",
            current,
            latest,
            if notes_text.is_empty() { "• Улучшения стабильности и новые функции\n".into() } else { notes_text }
        )
    } else {
        format!(
            "✅ <b>У вас установлена актуальная версия!</b>\n\n\
            📟 Версия: <code>{}</code>\n\
            Панель управления XKeen Route работает штатно и не требует обновлений.",
            current
        )
    };

    let mut btns = Vec::new();
    if update_available {
        btns.push(vec![InlineKeyboardButton {
            text: "⚡ Обновить роутер в 1 клик".into(),
            callback_data: "cb:update_install".into(),
        }]);
    }
    btns.push(vec![
        InlineKeyboardButton { text: "🔄 Проверить снова".into(), callback_data: "cb:update_check".into() },
        InlineKeyboardButton { text: "◀️ В меню".into(), callback_data: "cb:menu".into() },
    ]);

    let kb = InlineKeyboardMarkup { inline_keyboard: btns };
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
    let is_direct = cfg.direct_domains.iter().any(|d| d.eq_ignore_ascii_case(domain));

    let status_str = if is_force {
        "🛡️ <b>В списке Force-Proxy (всегда через VPN)</b>"
    } else if is_direct {
        "🌐 <b>В списке Direct (напрямую без VPN)</b>"
    } else {
        "⚪ <b>Стандартный маршрут (по правилам Mihomo)</b>"
    };

    let text = format!(
        "🌐 <b>Маршрутизация домена:</b> <code>{}</code>\n\n\
        • Статус: {}\n\n\
        <i>Выберите действие для изменения маршрутизации трафика:</i>",
        domain, status_str
    );

    let mut buttons = Vec::new();
    if !is_force {
        buttons.push(vec![InlineKeyboardButton {
            text: "➕ Добавить в Force-Proxy".into(),
            callback_data: format!("add_force:{}", domain),
        }]);
    }
    if !is_direct {
        buttons.push(vec![InlineKeyboardButton {
            text: "🚫 Добавить в Direct".into(),
            callback_data: format!("add_direct:{}", domain),
        }]);
    }
    if is_force || is_direct {
        buttons.push(vec![InlineKeyboardButton {
            text: "🗑️ Удалить из правил".into(),
            callback_data: format!("del_rule:{}", domain),
        }]);
    }
    buttons.push(vec![InlineKeyboardButton {
        text: "◀️ В меню".into(),
        callback_data: "cb:menu".into(),
    }]);

    let kb = InlineKeyboardMarkup { inline_keyboard: buttons };
    let _ = send_msg(http, token, chat_id, &text, Some(json!(kb))).await;
}

/// Отправка резервной копии config.json в виде файла в Telegram
async fn handle_backup_request(
    state: &AppState,
    http: &reqwest::Client,
    token: &str,
    chat_id: &str,
) {
    let _ = send_msg(http, token, chat_id, "⏳ <i>Формирование резервной копии конфигурации...</i>", None).await;

    match tokio::fs::read(&*state.config_path).await {
        Ok(bytes) => {
            let filename = format!("xkeen-route-config-{}.json", chrono::Local::now().format("%Y%m%d-%H%M"));
            let caption = format!(
                "💾 <b>Резервная копия XKeen Route</b>\n\n\
                📁 Файл: <code>{}</code>\n\
                📦 Размер: {} байт\n\
                ⏰ Создано: {}",
                filename,
                bytes.len(),
                chrono::Local::now().format("%d.%m.%Y %H:%M:%S")
            );
            if let Err(e) = send_doc(http, token, chat_id, &filename, bytes, &caption).await {
                let _ = send_msg(http, token, chat_id, &format!("❌ Не удалось отправить документ: {e}"), None).await;
            }
        }
        Err(e) => {
            let _ = send_msg(http, token, chat_id, &format!("❌ Не удалось прочитать config.json: {e}"), None).await;
        }
    }
}

// ----------------- Вспомогательные функции изменения конфигурации -----------------

async fn bot_toggle_adblock(state: &AppState) -> Result<bool, String> {
    let mut tx = crate::transaction::ConfigTx::begin(state).await?;
    let new_val = !tx.config().adblock_enabled;
    tx.config_mut().adblock_enabled = new_val;

    let _ = crate::adguard::set_protection(&state.http, &tx.config().adguard, new_val).await;

    let raw_yaml = tx.read_yaml().await?;
    let (new_yaml, _) = crate::routing::apply_routing(&raw_yaml, tx.config())
        .map_err(|e| format!("Ошибка роутинга: {e}"))?;

    tx.set_yaml(new_yaml)?;
    tx.commit_and_reload().await?;
    Ok(new_val)
}

async fn bot_add_force_domain(state: &AppState, domain: &str) -> Result<(), String> {
    let mut tx = crate::transaction::ConfigTx::begin(state).await?;
    tx.config_mut().direct_domains.retain(|d| !d.eq_ignore_ascii_case(domain));
    if !tx.config().force_domains.iter().any(|d| d.eq_ignore_ascii_case(domain)) {
        tx.config_mut().force_domains.push(domain.to_string());
    }
    let raw_yaml = tx.read_yaml().await?;
    let (new_yaml, _) = crate::routing::apply_routing(&raw_yaml, tx.config())
        .map_err(|e| format!("Ошибка роутинга: {e}"))?;
    tx.set_yaml(new_yaml)?;
    tx.commit_and_reload().await?;
    let all_force = tx.config().force_domains.clone();
    let _ = crate::override_sync::sync_geo_override(&all_force).await;
    Ok(())
}

async fn bot_add_direct_domain(state: &AppState, domain: &str) -> Result<(), String> {
    let mut tx = crate::transaction::ConfigTx::begin(state).await?;
    tx.config_mut().force_domains.retain(|d| !d.eq_ignore_ascii_case(domain));
    if !tx.config().direct_domains.iter().any(|d| d.eq_ignore_ascii_case(domain)) {
        tx.config_mut().direct_domains.push(domain.to_string());
    }
    let raw_yaml = tx.read_yaml().await?;
    let (new_yaml, _) = crate::routing::apply_routing(&raw_yaml, tx.config())
        .map_err(|e| format!("Ошибка роутинга: {e}"))?;
    tx.set_yaml(new_yaml)?;
    tx.commit_and_reload().await?;
    let all_force = tx.config().force_domains.clone();
    let _ = crate::override_sync::sync_geo_override(&all_force).await;
    Ok(())
}

async fn bot_remove_domain_rules(state: &AppState, domain: &str) -> Result<(), String> {
    let mut tx = crate::transaction::ConfigTx::begin(state).await?;
    tx.config_mut().force_domains.retain(|d| !d.eq_ignore_ascii_case(domain));
    tx.config_mut().direct_domains.retain(|d| !d.eq_ignore_ascii_case(domain));
    let raw_yaml = tx.read_yaml().await?;
    let (new_yaml, _) = crate::routing::apply_routing(&raw_yaml, tx.config())
        .map_err(|e| format!("Ошибка роутинга: {e}"))?;
    tx.set_yaml(new_yaml)?;
    tx.commit_and_reload().await?;
    let all_force = tx.config().force_domains.clone();
    let _ = crate::override_sync::sync_geo_override(&all_force).await;
    Ok(())
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

async fn send_doc(
    http: &reqwest::Client,
    token: &str,
    chat_id: &str,
    filename: &str,
    data: Vec<u8>,
    caption: &str,
) -> Result<(), String> {
    let url = format!("https://api.telegram.org/bot{}/sendDocument", token);
    let part = reqwest::multipart::Part::bytes(data)
        .file_name(filename.to_string())
        .mime_str("application/json")
        .map_err(|e| e.to_string())?;

    let form = reqwest::multipart::Form::new()
        .text("chat_id", chat_id.to_string())
        .text("caption", caption.to_string())
        .text("parse_mode", "HTML")
        .part("document", part);

    let res = http
        .post(&url)
        .header("User-Agent", "xkeen-route")
        .multipart(form)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if res.status().is_success() {
        Ok(())
    } else {
        Err(format!("HTTP {}", res.status()))
    }
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

    #[test]
    fn test_format_devices_report_str() {
        let devices = vec![
            crate::rci::Device {
                mac: "aa:bb:cc:dd:ee:01".into(),
                name: "Work-PC".into(),
                ip: "192.168.2.10".into(),
                policy: "".into(),
                policy_name: "".into(),
                online: true,
                interface: "Bridge0".into(),
                is_current_device: false,
                rxbytes: 0,
                txbytes: 0,
                speed_limit_kbps: 0,
                ipv6: Vec::new(),
            },
            crate::rci::Device {
                mac: "aa:bb:cc:dd:ee:02".into(),
                name: "Old-Phone".into(),
                ip: "192.168.2.55".into(),
                policy: "".into(),
                policy_name: "".into(),
                online: false,
                interface: "Bridge0".into(),
                is_current_device: false,
                rxbytes: 0,
                txbytes: 0,
                speed_limit_kbps: 0,
                ipv6: Vec::new(),
            },
        ];

        let report = format_devices_report_str(&devices);
        assert!(report.contains("Work-PC"));
        assert!(report.contains("192.168.2.10"));
        assert!(report.contains("Old-Phone"));
        assert!(report.contains("Онлайн: 🟢 1"));
    }

    #[test]
    fn test_format_speedtest_report_str() {
        let resp = crate::speedtest::SpeedtestResponse {
            server_id: "srv1".into(),
            latency_ms: 35,
            speed_mbps: 87.5,
            bytes_downloaded: 10 * 1024 * 1024,
            duration_secs: 1.2,
            verified: true,
        };

        let report = format_speedtest_report_str("🇳🇱 Нидерланды", &resp);
        assert!(report.contains("Нидерланды"));
        assert!(report.contains("87.5 Мбит/с"));
        assert!(report.contains("35 мс"));
        assert!(report.contains("10.0 МБ"));
    }

    #[test]
    fn test_format_history_report_str() {
        let events = vec![
            crate::failover::FailoverEvent {
                time: "15:30:00".into(),
                message: "Успешная проверка узла".into(),
                switched: false,
            },
            crate::failover::FailoverEvent {
                time: "15:31:00".into(),
                message: "Переключение на Резервный".into(),
                switched: true,
            },
        ];

        let report = format_history_report_str(&events);
        assert!(report.contains("15:30:00"));
        assert!(report.contains("Успешная проверка узла"));
        assert!(report.contains("15:31:00"));
        assert!(report.contains("⚠️"));
    }
}
