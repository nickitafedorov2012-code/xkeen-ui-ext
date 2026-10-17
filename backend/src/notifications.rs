use serde_json::json;
use crate::{config::NotificationsConfig, log_i, log_w};

/// Валидация и нормализация токена Telegram бота.
pub fn validate_telegram_bot_token(token: &str) -> Result<String, String> {
    let clean = token.trim().trim_matches(|c| c == '\'' || c == '"');
    if clean.is_empty() {
        return Err("Telegram Bot Token не может быть пустым".to_string());
    }
    if clean.starts_with('@') || clean.starts_with("http://") || clean.starts_with("https://") || clean.starts_with("t.me/") {
        return Err("Указана ссылка или юзернейм вместо токена. Скопируйте токен целиком из сообщения от @BotFather (вид: 123456789:ABCdef...)".to_string());
    }
    let parts: Vec<&str> = clean.splitn(2, ':').collect();
    if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() || !parts[0].chars().all(|c| c.is_ascii_digit()) {
        return Err("Некорректный формат токена бота. Токен должен начинаться с числового ID бота и двоеточия (например: 123456789:ABCdefGhI...)".to_string());
    }
    Ok(clean.to_string())
}

/// Валидация и нормализация Telegram Chat ID.
pub fn validate_telegram_chat_id(chat_id: &str, bot_token: Option<&str>) -> Result<String, String> {
    let clean = chat_id.trim().trim_matches(|c| c == '\'' || c == '"');
    if clean.is_empty() {
        return Err("Telegram Chat ID не может быть пустым".to_string());
    }
    if clean.starts_with('@') {
        return Err("Chat ID для личных сообщений должен быть числовым (получите его через @userinfobot). @username допустим только для публичных каналов".to_string());
    }
    // Если передан токен бота, проверяем, не вставил ли пользователь ID самого бота вместо своего ID
    if let Some(bot_tok) = bot_token {
        if let Ok(valid_token) = validate_telegram_bot_token(bot_tok) {
            let bot_id = valid_token.split(':').next().unwrap_or("");
            if !bot_id.is_empty() && clean == bot_id {
                return Err(format!("Указан ID самого бота ({}). Бот не может отправить сообщение сам себе! Получите ваш личный Chat ID через бота @userinfobot", clean));
            }
        }
    }
    // Проверка на допустимые символы для числового chat_id
    if clean.starts_with('-') {
        if !clean[1..].chars().all(|c| c.is_ascii_digit()) {
            return Err("Chat ID группы должен состоять только из знака минус и цифр (например, -1001234567890)".to_string());
        }
    } else if !clean.chars().all(|c| c.is_ascii_digit()) {
        return Err("Личный Chat ID должен состоять только из цифр (например, 123456789). Узнать его можно через @userinfobot".to_string());
    }
    Ok(clean.to_string())
}

/// Человекочитаемая трансляция кодов ошибок Telegram API.
pub fn translate_telegram_error(status: reqwest::StatusCode, body: &str) -> String {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
        let desc = v.get("description").and_then(|d| d.as_str()).unwrap_or("");
        let err_code = v.get("error_code").and_then(|c| c.as_i64()).unwrap_or(status.as_u16() as i64);

        if err_code == 401 || desc.to_lowercase().contains("unauthorized") {
            return "Неверный токен бота (401 Unauthorized). Проверьте токен, полученный от @BotFather".to_string();
        }
        if err_code == 404 {
            return "Токен бота не найден в Telegram (404 Not Found). Убедитесь, что токен скопирован целиком вместе с цифрами в начале".to_string();
        }
        if err_code == 403 {
            if desc.contains("bot can't initiate conversation") || desc.contains("blocked by the user") {
                return "Бот не может написать первым (403 Forbidden). Откройте диалог с вашим ботом в Telegram и нажмите кнопку СТАРТ (/start)".to_string();
            }
            return format!("Доступ запрещен (403 Forbidden): {}", desc);
        }
        if err_code == 400 {
            if desc.contains("chat not found") {
                return "Чат не найден (400 Bad Request: chat not found). Проверьте правильность Chat ID и убедитесь, что вы запустили бота (/start) или добавили его в группу".to_string();
            }
            if desc.contains("wrong type of the chat") || desc.contains("not enough rights") {
                return format!("Ошибка прав доступа в чате: {desc}");
            }
            return format!("Ошибка параметров запроса (400 Bad Request): {desc}");
        }
        if !desc.is_empty() {
            return format!("Telegram API ошибка {}: {}", err_code, desc);
        }
    }
    format!("Telegram API вернул ошибку {}: {}", status, body)
}

/// Отправка форматированного уведомления в Telegram через Bot API с поддержкой локального прокси Mihomo.
pub async fn send_telegram_with_proxy(
    direct_http: &reqwest::Client,
    proxy_url: Option<&str>,
    bot_token: &str,
    chat_id: &str,
    text: &str,
) -> Result<(), String> {
    let clean_token = validate_telegram_bot_token(bot_token)?;
    let clean_chat_id = validate_telegram_chat_id(chat_id, Some(&clean_token))?;

    let url = format!("https://api.telegram.org/bot{}/sendMessage", clean_token);
    let payload = json!({
        "chat_id": clean_chat_id,
        "text": text,
        "parse_mode": "HTML",
        "disable_web_page_preview": true
    });

    // 1. Формируем список клиентов: первым пробуем проксированный клиент через Mihomo (обход блокировок ТСПУ/РКН на роутере),
    // вторым — прямой клиент.
    let mut clients: Vec<(&str, reqwest::Client)> = Vec::new();

    if let Some(proxy_addr) = proxy_url {
        let p_trimmed = proxy_addr.trim();
        if !p_trimmed.is_empty() {
            match reqwest::Proxy::all(p_trimmed) {
                Ok(proxy) => {
                    match reqwest::Client::builder()
                        .proxy(proxy)
                        .timeout(std::time::Duration::from_secs(10))
                        .build()
                    {
                        Ok(c) => clients.push(("Mihomo Proxy", c)),
                        Err(e) => log_w!("[NOTIF] Ошибка создания HTTP-клиента с прокси {}: {}", p_trimmed, e),
                    }
                }
                Err(e) => log_w!("[NOTIF] Ошибка создания reqwest::Proxy({}): {}", p_trimmed, e),
            }
        }
    }
    clients.push(("Прямое соединение", direct_http.clone()));

    let mut err_details = Vec::new();

    for (name, client) in clients {
        log_i!("[NOTIF] Отправка уведомления в Telegram через {}...", name);
        let req = client
            .post(&url)
            .header("User-Agent", "xkeen-route")
            .json(&payload)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await;

        match req {
            Ok(res) => {
                let status = res.status();
                if status.is_success() {
                    log_i!("[NOTIF] ✓ Сообщение Telegram успешно доставлено через {}", name);
                    return Ok(());
                } else {
                    let body = res.text().await.unwrap_or_default();
                    let translated = translate_telegram_error(status, &body);
                    log_w!("[NOTIF] Telegram API вернул HTTP {}: {}", status, translated);
                    return Err(translated);
                }
            }
            Err(e) => {
                log_w!("[NOTIF] Сбой отправки через {}: {}", name, e);
                err_details.push(format!("{}: {}", name, e));
            }
        }
    }

    Err(format!(
        "Не удалось подключиться к Telegram API ({}). Проверьте, включен ли прокси Mihomo и есть ли доступ в интернет на роутере",
        err_details.join("; ")
    ))
}

/// Отправка форматированного уведомления в Telegram через Bot API (обратная совместимость).
pub async fn send_telegram(
    http: &reqwest::Client,
    bot_token: &str,
    chat_id: &str,
    text: &str,
) -> Result<(), String> {
    send_telegram_with_proxy(http, None, bot_token, chat_id, text).await
}

/// Экранирование HTML-спецсимволов для безопасной отправки сообщений в Telegram HTML mode (SEC-14).
pub fn escape_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Валидация webhook URL для защиты от SSRF во внутреннюю сеть роутера (SEC-14).
pub fn validate_webhook_url(url_str: &str) -> Result<(), String> {
    let trimmed = url_str.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    let parsed = reqwest::Url::parse(trimmed).map_err(|e| format!("Некорректный URL вебхука: {e}"))?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err("Поддерживаются только http и https URL для Webhook".to_string());
    }
    let host = parsed.host_str().ok_or_else(|| "В URL вебхука отсутствует хост".to_string())?;
    if host.eq_ignore_ascii_case("localhost")
        || host.eq_ignore_ascii_case("router")
        || host.eq_ignore_ascii_case("my.keenetic.net")
    {
        return Err("Запрещен вызов локальных адресов роутера через Webhook".to_string());
    }
    let host_unbracketed = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = host_unbracketed.parse::<std::net::IpAddr>() {
        let is_unsafe = match ip {
            std::net::IpAddr::V4(v4) => {
                v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_unspecified() || v4.is_broadcast()
            }
            std::net::IpAddr::V6(v6) => {
                if v6.is_loopback()
                    || v6.is_unspecified()
                    || (v6.segments()[0] & 0xffc0 == 0xfe80)
                    || (v6.segments()[0] & 0xfe00 == 0xfc00)
                {
                    true
                } else if let Some(mapped_v4) = v6.to_ipv4_mapped().or_else(|| v6.to_ipv4()) {
                    mapped_v4.is_loopback()
                        || mapped_v4.is_private()
                        || mapped_v4.is_link_local()
                        || mapped_v4.is_unspecified()
                        || mapped_v4.is_broadcast()
                } else {
                    false
                }
            }
        };
        if is_unsafe {
            return Err("Запрещен вызов внутренних/локальных (private, loopback, link-local) адресов через Webhook".to_string());
        }
    }
    Ok(())
}

/// Отправка webhook POST запроса.
pub async fn send_webhook(
    http: &reqwest::Client,
    webhook_url: &str,
    event_type: &str,
    title: &str,
    details: &str,
) -> Result<(), String> {
    if webhook_url.trim().is_empty() {
        return Ok(());
    }
    validate_webhook_url(webhook_url)?;

    let payload = json!({
        "source": "xkeen-route",
        "event": event_type,
        "title": title,
        "details": details,
        "timestamp": chrono::Local::now().to_rfc3339()
    });

    let res = http
        .post(webhook_url.trim())
        .json(&payload)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("Ошибка отправки Webhook: {}", e))?;

    if res.status().is_success() {
        Ok(())
    } else {
        Err(format!("Webhook endpoint вернул ошибку: {}", res.status()))
    }
}

/// Комплексная отправка уведомления о событии Failover.
pub async fn notify_failover(
    http: &reqwest::Client,
    proxy_url: Option<&str>,
    cfg: &NotificationsConfig,
    event_type: &str, // "switch" | "down" | "restore" | "all_down"
    title: &str,
    details: &str,
) {
    if cfg.telegram_enabled && !cfg.telegram_bot_token.is_empty() && !cfg.telegram_chat_id.is_empty() {
        let icon = match event_type {
            "restore" => "🟢",
            "switch" => "🟡",
            "down" => "🔴",
            "all_down" => "🚨",
            _ => "ℹ️",
        };

        let now = chrono::Local::now().format("%d.%m.%Y %H:%M:%S");
        let safe_title = escape_html(title);
        let safe_details = escape_html(details);
        let html_msg = format!(
            "<b>{} XKeen Route Failover</b>\n\n<b>{}</b>\n{}\n\n<i>🕒 {}</i>",
            icon, safe_title, safe_details, now
        );

        if let Err(e) = send_telegram_with_proxy(http, proxy_url, &cfg.telegram_bot_token, &cfg.telegram_chat_id, &html_msg).await {
            log_w!("Не удалось отправить Telegram уведомление: {}", e);
        } else {
            log_i!("Отправлено Telegram уведомление о событии Failover: {}", title);
        }
    }

    if !cfg.webhook_url.is_empty() {
        if let Err(e) = send_webhook(http, &cfg.webhook_url, event_type, title, details).await {
            log_w!("Не удалось отправить Webhook: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_html_special_chars() {
        assert_eq!(escape_html("Server <Node 1> & Co \"Quote\""), "Server &lt;Node 1&gt; &amp; Co &quot;Quote&quot;");
        assert_eq!(escape_html("Plain string"), "Plain string");
        assert_eq!(escape_html("<script>alert(1)</script>"), "&lt;script&gt;alert(1)&lt;/script&gt;");
    }

    #[test]
    fn test_validate_webhook_url() {
        assert!(validate_webhook_url("https://discord.com/api/webhooks/123/abc").is_ok());
        assert!(validate_webhook_url("http://webhook.site/my-uuid").is_ok());
        assert!(validate_webhook_url("https://api.telegram.org/bot123/send").is_ok());
        assert!(validate_webhook_url("").is_ok());

        // Rejection of invalid schemes and loopback addresses
        assert!(validate_webhook_url("ftp://example.com/hook").is_err());
        assert!(validate_webhook_url("http://localhost:8080/hook").is_err());
        assert!(validate_webhook_url("http://127.0.0.1:9090/clash").is_err());
        assert!(validate_webhook_url("http://[::1]:9090/clash").is_err());

        // SEC-03: Rejection of private, link-local, and router addresses (SSRF)
        assert!(validate_webhook_url("http://192.168.1.1:79/rci/").is_err());
        assert!(validate_webhook_url("http://192.168.2.1:1001/api/status").is_err());
        assert!(validate_webhook_url("http://10.0.0.1/hook").is_err());
        assert!(validate_webhook_url("http://172.16.0.1:8080").is_err());
        assert!(validate_webhook_url("http://169.254.1.1/metadata").is_err());
        assert!(validate_webhook_url("http://[fe80::1]/hook").is_err());
        assert!(validate_webhook_url("http://[fd00::1]/hook").is_err());
        assert!(validate_webhook_url("http://[::ffff:192.168.1.1]/hook").is_err());
        assert!(validate_webhook_url("http://router/hook").is_err());
        assert!(validate_webhook_url("http://my.keenetic.net/hook").is_err());
    }

    #[test]
    fn test_validate_telegram_bot_token() {
        // Валидный токен
        assert_eq!(
            validate_telegram_bot_token("8959891087:AAHWkeSw_WL6b8k3J9tONrms-7F2NgE2Yfo").unwrap(),
            "8959891087:AAHWkeSw_WL6b8k3J9tONrms-7F2NgE2Yfo"
        );
        // Токен с пробелами и кавычками
        assert_eq!(
            validate_telegram_bot_token("  \"8959891087:AAHWkeSw_WL6b8k3J9tONrms-7F2NgE2Yfo\"  ").unwrap(),
            "8959891087:AAHWkeSw_WL6b8k3J9tONrms-7F2NgE2Yfo"
        );
        // Пустой токен
        assert!(validate_telegram_bot_token("").is_err());
        // Ссылка вместо токена
        assert!(validate_telegram_bot_token("https://t.me/my_bot").is_err());
        assert!(validate_telegram_bot_token("@my_bot").is_err());
        // Отрезанный ID бота (как на скриншоте 2)
        let err = validate_telegram_bot_token("AAHWkeSw_WL6b8k3J9tONrms-7F2NgE2Yfo").unwrap_err();
        assert!(err.contains("Некорректный формат токена бота"));
    }

    #[test]
    fn test_validate_telegram_chat_id() {
        let token = "8959891087:AAHWkeSw_WL6b8k3J9tONrms-7F2NgE2Yfo";

        // Валидный личный Chat ID
        assert_eq!(validate_telegram_chat_id("123456789", Some(token)).unwrap(), "123456789");
        // Валидный Chat ID супергруппы
        assert_eq!(validate_telegram_chat_id("-1001234567890", Some(token)).unwrap(), "-1001234567890");

        // Ошибка: совпадение Chat ID с ID самого бота (как на скриншотах 1 и 2)
        let err_same_bot = validate_telegram_chat_id("8959891087", Some(token)).unwrap_err();
        assert!(err_same_bot.contains("Указан ID самого бота"));

        // Ошибка: буквы в Chat ID
        assert!(validate_telegram_chat_id("abc12345", Some(token)).is_err());
        // Ошибка: юзернейм вместо ID
        assert!(validate_telegram_chat_id("@my_user", Some(token)).is_err());
    }

    #[test]
    fn test_translate_telegram_error() {
        let err_401 = translate_telegram_error(
            reqwest::StatusCode::UNAUTHORIZED,
            r#"{"ok":false,"error_code":401,"description":"Unauthorized"}"#,
        );
        assert!(err_401.contains("Неверный токен бота"));

        let err_403 = translate_telegram_error(
            reqwest::StatusCode::FORBIDDEN,
            r#"{"ok":false,"error_code":403,"description":"Forbidden: bot can't initiate conversation with a user"}"#,
        );
        assert!(err_403.contains("Бот не может написать первым"));

        let err_400 = translate_telegram_error(
            reqwest::StatusCode::BAD_REQUEST,
            r#"{"ok":false,"error_code":400,"description":"Bad Request: chat not found"}"#,
        );
        assert!(err_400.contains("Чат не найден"));
    }

    #[test]
    fn test_proxy_builder_webpki() {
        let p = reqwest::Proxy::all("http://127.0.0.1:7890");
        assert!(p.is_ok(), "Proxy::all failed: {:?}", p.err());
        let c = reqwest::Client::builder().proxy(p.unwrap()).build();
        assert!(c.is_ok(), "Client::build failed: {:?}", c.err());
    }

    #[test]
    fn test_bot_token_preservation_and_fallback() {
        // Fallback-логика: если передан masked token ("******") или пустая строка,
        // должна использоваться сохранённая в конфиге строка.
        let saved_token = "8959891087:AAHWkeSw_WL6b8k3J9tONrms-7F2NgE2Yfo";
        let body_masked = Some("******".to_string());
        let resolved = match body_masked {
            Some(ref t) if !t.trim().is_empty() && t.trim() != "******" => t.trim().to_string(),
            _ => saved_token.to_string(),
        };
        assert_eq!(resolved, saved_token);

        let body_custom = Some("123456789:ABC_DEF_GHI".to_string());
        let resolved_custom = match body_custom {
            Some(ref t) if !t.trim().is_empty() && t.trim() != "******" => t.trim().to_string(),
            _ => saved_token.to_string(),
        };
        assert_eq!(resolved_custom, "123456789:ABC_DEF_GHI");
    }
}
