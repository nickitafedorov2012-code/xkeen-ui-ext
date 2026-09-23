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

/// Выполнение замера скорости загрузки через выбранный прокси-сервер.
pub async fn run_speedtest(
    _http: &reqwest::Client,
    _cfg: &AppConfig,
    server_id: &str,
) -> Result<SpeedtestResponse, String> {
    log_i!("Запуск теста скорости для сервера '{}'", server_id);

    // Тестовые зеркала для скачивания чанка (5-10 МБ)
    let test_urls = [
        "https://speed.cloudflare.com/__down?bytes=5242880",
        "http://cachefly.cachefly.net/5mb.test",
        "https://proof.ovh.net/files/1Mb.dat",
    ];

    // Настраиваем HTTP клиент через локальный HTTP/SOCKS5 прокси Mihomo
    let proxy_url = _cfg.mihomo_proxy_url();
    let proxy = reqwest::Proxy::all(&proxy_url)
        .map_err(|e| format!("Ошибка создания прокси: {}", e))?;

    let client = reqwest::Client::builder()
        .proxy(proxy)
        .danger_accept_invalid_certs(true)
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| format!("Ошибка инициализации HTTP клиента: {}", e))?;

    let start = Instant::now();
    let mut total_bytes = 0u64;
    let mut first_byte_ms = 0u32;
    let mut success = false;

    for url in &test_urls {
        let req_start = Instant::now();
        match client.get(*url).send().await {
            Ok(mut resp) => {
                if !resp.status().is_success() {
                    continue;
                }
                first_byte_ms = req_start.elapsed().as_millis() as u32;

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

    if !success || total_bytes == 0 {
        return Err("Не удалось выполнить тест скорости: тестовые узлы недоступны".to_string());
    }

    let duration_secs = start.elapsed().as_secs_f64();
    let speed_mbps = ((total_bytes as f64 * 8.0) / (duration_secs * 1_000_000.0) * 100.0).round() / 100.0;

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
