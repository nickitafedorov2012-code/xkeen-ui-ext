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
    if host.eq_ignore_ascii_case("localhost") {
        return Err("Запрещен вызов localhost через Webhook".to_string());
    }
    let host_unbracketed = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = host_unbracketed.parse::<std::net::IpAddr>() {
        let is_unsafe = match ip {
            std::net::IpAddr::V4(v4) => v4.is_loopback() || v4.is_link_local() || v4.is_unspecified(),
            std::net::IpAddr::V6(v6) => v6.is_loopback() || v6.is_unspecified(),
        };
        if is_unsafe {
            return Err("Запрещен вызов loopback/link-local адресов через Webhook".to_string());
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
        assert!(validate_webhook_url("").is_ok());

        // Rejection of invalid schemes and loopback addresses
        assert!(validate_webhook_url("ftp://example.com/hook").is_err());
        assert!(validate_webhook_url("http://localhost:8080/hook").is_err());
        assert!(validate_webhook_url("http://127.0.0.1:9090/clash").is_err());
        assert!(validate_webhook_url("http://[::1]:9090/clash").is_err());
    }
}
