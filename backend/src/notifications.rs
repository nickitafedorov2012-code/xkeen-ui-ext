use serde_json::json;
use crate::{config::NotificationsConfig, log_i, log_w};

/// Отправка форматированного уведомления в Telegram через Bot API.
pub async fn send_telegram(
    http: &reqwest::Client,
    bot_token: &str,
    chat_id: &str,
    text: &str,
) -> Result<(), String> {
    if bot_token.trim().is_empty() || chat_id.trim().is_empty() {
        return Err("Не задан Telegram Bot Token или Chat ID".to_string());
    }

    let url = format!("https://api.telegram.org/bot{}/sendMessage", bot_token.trim());
    let payload = json!({
        "chat_id": chat_id.trim(),
        "text": text,
        "parse_mode": "HTML",
        "disable_web_page_preview": true
    });

    let res = http
        .post(&url)
        .json(&payload)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("Ошибка отправки в Telegram: {}", e))?;

    if res.status().is_success() {
        Ok(())
    } else {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        Err(format!("Telegram API вернул ошибку {}: {}", status, body))
    }
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
        let html_msg = format!(
            "<b>{} XKeen Route Failover</b>\n\n<b>{}</b>\n{}\n\n<i>🕒 {}</i>",
            icon, title, details, now
        );

        if let Err(e) = send_telegram(http, &cfg.telegram_bot_token, &cfg.telegram_chat_id, &html_msg).await {
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
