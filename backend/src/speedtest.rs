use serde::{Deserialize, Serialize};
use std::time::Instant;
use crate::{config::AppConfig, log_i, log_w};

#[derive(Deserialize)]
pub struct SpeedtestRequest {
    pub server_id: String,
}

#[derive(Serialize)]
pub struct SpeedtestResponse {
    pub server_id: String,
    pub latency_ms: u32,
    pub speed_mbps: f64,
    pub bytes_downloaded: u64,
    pub duration_secs: f64,
}

async fn restore_server_if_active(
    http: &reqwest::Client,
    cfg: &AppConfig,
    original: Option<&str>,
    tested_server: &str,
) {
    if let Some(orig) = original {
        if !orig.is_empty() && orig != tested_server {
            if let Ok(current_proxies) = crate::mihomo::get_proxies(http, cfg).await {
                let current_leaf = crate::mihomo::resolve_active_leaf(&current_proxies);
                if current_leaf == tested_server {
                    let _ = crate::mihomo::switch_server(http, cfg, orig).await;
                } else {
                    log_i!("Восстановление сервера после speedtest отменено: активный сервер был переключен во время замера ('{}')", current_leaf);
                }
            } else {
                let _ = crate::mihomo::switch_server(http, cfg, orig).await;
            }
        }
    }
}

/// Выполнение замера скорости загрузки через выбранный прокси-сервер.
pub async fn run_speedtest(
    _http: &reqwest::Client,
    _cfg: &AppConfig,
    server_id: &str,
) -> Result<SpeedtestResponse, String> {
    log_i!("Запуск теста скорости для сервера '{}'", server_id);

    // 1. Замер задержки выбранного узла через API ядра Mihomo
    let mut first_byte_ms = 0u32;
    if !server_id.is_empty() {
        let measured_ping = crate::mihomo::ping_server_url(_http, _cfg, server_id, 4000, Some("https://cp.cloudflare.com")).await;
        if measured_ping > 0 {
            first_byte_ms = measured_ping as u32;
        }
    }

    // 2. Временное переключение активного сервера на целевой server_id для замера
    let proxies_opt = crate::mihomo::get_proxies(_http, _cfg).await.ok();
    let original_server = proxies_opt.as_ref().map(|p| crate::mihomo::resolve_active_leaf(p));
    let need_restore = if let Some(orig) = &original_server {
        if !orig.is_empty() && orig != server_id && !server_id.is_empty() {
            crate::mihomo::switch_server(_http, _cfg, server_id).await.is_ok()
        } else {
            false
        }
    } else {
        false
    };

    // Тестовые зеркала для скачивания чанка (5-10 МБ)
    let test_urls = [
        "https://speed.cloudflare.com/__down?bytes=5242880",
        "http://cachefly.cachefly.net/5mb.test",
        "https://proof.ovh.net/files/1Mb.dat",
    ];

    // Настраиваем HTTP клиент через локальный HTTP/SOCKS5 прокси Mihomo
    let proxy_url = _cfg.mihomo_proxy_url();
    let proxy = match reqwest::Proxy::all(&proxy_url) {
        Ok(p) => p,
        Err(e) => {
            if need_restore {
                restore_server_if_active(_http, _cfg, original_server.as_deref(), server_id).await;
            }
            return Err(format!("Ошибка создания прокси: {}", e));
        }
    };

    let client = match reqwest::Client::builder()
        .proxy(proxy)
        .timeout(std::time::Duration::from_secs(20))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            if need_restore {
                restore_server_if_active(_http, _cfg, original_server.as_deref(), server_id).await;
            }
            return Err(format!("Ошибка инициализации HTTP клиента: {}", e));
        }
    };

    let start = Instant::now();
    let mut total_bytes = 0u64;
    let mut success = false;

    for url in &test_urls {
        let req_start = Instant::now();
        match client.get(*url).send().await {
            Ok(mut resp) => {
                if !resp.status().is_success() {
                    continue;
                }
                if first_byte_ms == 0 {
                    first_byte_ms = req_start.elapsed().as_millis() as u32;
                }

                while let Ok(Some(chunk)) = resp.chunk().await {
                    total_bytes += chunk.len() as u64;
                    // Если скачали больше 5 МБ или прошло больше 8 секунд — завершаем замер
                    if total_bytes >= 5 * 1024 * 1024 || start.elapsed().as_secs() >= 8 {
                        break;
                    }
                }
                if total_bytes > 100_000 {
                    success = true;
                    break;
                }
            }
            Err(e) => {
                log_w!("Ошибка скачивания с {}: {}", url, e);
            }
        }
    }

    // Восстанавливаем исходный сервер после замера только если активным всё ещё является тестируемый узел
    if need_restore {
        restore_server_if_active(_http, _cfg, original_server.as_deref(), server_id).await;
    }

    if !success || total_bytes == 0 {
        return Err("Не удалось выполнить тест скорости: тестовые узлы недоступны".to_string());
    }

    let duration_secs = start.elapsed().as_secs_f64().max(0.001);
    let speed_mbps = calculate_speed(total_bytes, duration_secs);

    log_i!(
        "Тест скорости для '{}' завершён: {} Мбит/с ({} байт за {:.2} с, отклик {} мс)",
        server_id,
        speed_mbps,
        total_bytes,
        duration_secs,
        first_byte_ms
    );

    Ok(SpeedtestResponse {
        server_id: server_id.to_string(),
        latency_ms: first_byte_ms,
        speed_mbps,
        bytes_downloaded: total_bytes,
        duration_secs: (duration_secs * 100.0).round() / 100.0,
    })
}

/// Расчет скорости в Мбит/с с защитой от деления на ноль.
pub fn calculate_speed(total_bytes: u64, duration_secs: f64) -> f64 {
    let dur = duration_secs.max(0.001);
    ((total_bytes as f64 * 8.0) / (dur * 1_000_000.0) * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_speed_normal() {
        let speed = calculate_speed(5242880, 2.0);
        assert_eq!(speed, 20.97);
    }

    #[test]
    fn test_calculate_speed_zero_duration_safe() {
        let speed = calculate_speed(1_000_000, 0.0);
        assert!(!speed.is_nan());
        assert!(!speed.is_infinite());
        assert!(speed > 0.0);
    }

    #[test]
    fn test_calculate_speed_zero_bytes() {
        let speed = calculate_speed(0, 1.0);
        assert_eq!(speed, 0.0);
    }
}
