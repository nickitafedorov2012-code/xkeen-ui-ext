use serde::{Deserialize, Serialize};
use std::time::Instant;
use std::collections::HashMap;
use serde_json::json;
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
    #[serde(default)]
    pub verified: bool,
}

/// Сохранение полного снимка выбора групп selector перед замером
pub async fn take_groups_snapshot(
    http: &reqwest::Client,
    cfg: &AppConfig,
) -> Result<(HashMap<String, String>, String), String> {
    let proxies = crate::mihomo::get_proxies(http, cfg).await
        .map_err(|e| format!("Ошибка получения списка прокси ядра Mihomo: {}", e))?;
    let active_leaf = crate::mihomo::resolve_active_leaf(&proxies);
    let mut snapshot = HashMap::new();
    for (name, val) in &proxies {
        let typ = val.get("type").and_then(|t| t.as_str()).unwrap_or("").to_lowercase();
        if typ == "selector" || typ == "select" {
            if let Some(now) = val.get("now").and_then(|n| n.as_str()) {
                snapshot.insert(name.clone(), now.to_string());
            }
        }
    }
    Ok((snapshot, active_leaf))
}

/// Восстановление точного снимка выбора групп Selector, сохраненного перед замером
pub async fn restore_groups_snapshot(
    http: &reqwest::Client,
    cfg: &AppConfig,
    snapshot: &HashMap<String, String>,
) {
    if snapshot.is_empty() {
        return;
    }
    log_i!("Восстановление снимка групп выбора после Speedtest ({} групп)", snapshot.len());
    for (group_name, orig_target) in snapshot {
        let _ = crate::mihomo::m_put(
            http,
            cfg,
            &format!("/proxies/{}", crate::mihomo::urlencoding_lite(group_name)),
            json!({ "name": orig_target }),
            3,
        ).await;
    }
}

/// Выполнение замера скорости загрузки через выбранный прокси-сервер.
pub async fn run_speedtest(
    _http: &reqwest::Client,
    _cfg: &AppConfig,
    server_id: &str,
) -> Result<SpeedtestResponse, String> {
    let target_server = server_id.trim();
    if target_server.is_empty() {
        return Err("Идентификатор сервера для Speedtest не может быть пустым".to_string());
    }

    log_i!("Запуск теста скорости для сервера '{}'", target_server);

    // 1. Создание снимка текущего выбора всех групп Selector ядра Mihomo
    let (group_snapshot, original_server) = take_groups_snapshot(_http, _cfg).await?;

    // 2. Переключение активного маршрута на запрошенный сервер (DIAG-01)
    let need_restore = if original_server != target_server {
        match crate::mihomo::switch_server(_http, _cfg, target_server).await {
            Ok(_) => true,
            Err(e) => return Err(format!("Не удалось переключить маршрут на запрошенный сервер '{}': {}", target_server, e)),
        }
    } else {
        false
    };

    // 3. Подтверждение тестового маршрута в runtime (DIAG-01)
    // Маршрут должен быть строго подтвержден в runtime:
    // либо активный leaf равен target_server, либо основной селектор (PROXY/GLOBAL/etc)
    // переключен на target_server (если target_server сам является группой или селектором)
    let current_proxies = match crate::mihomo::get_proxies(_http, _cfg).await {
        Ok(p) => p,
        Err(e) => {
            if need_restore {
                restore_groups_snapshot(_http, _cfg, &group_snapshot).await;
            }
            return Err(format!("Ошибка проверки прокси ядра Mihomo: {}", e));
        }
    };
    let current_leaf = crate::mihomo::resolve_active_leaf(&current_proxies);
    let is_confirmed = if current_leaf == target_server {
        true
    } else {
        // Проверяем только основные группы (PROXY, GLOBAL, Proxy, auto), исключая изолированные группы устройств
        ["PROXY", "GLOBAL", "Proxy", "auto"].iter().any(|&grp_name| {
            current_proxies.get(grp_name)
                .and_then(|g| g.get("now"))
                .and_then(|n| n.as_str())
                == Some(target_server)
        })
    };

    if !is_confirmed {
        if need_restore {
            restore_groups_snapshot(_http, _cfg, &group_snapshot).await;
        }
        return Err(format!(
            "Подтверждение тестового маршрута не удалось: сервер '{}' не стал активным в ядре Mihomo (текущий: '{}')",
            target_server,
            if current_leaf.is_empty() { "не определён" } else { &current_leaf }
        ));
    }

    // 4. Замер задержки выбранного узла через API ядра Mihomo
    let mut first_byte_ms = 0u32;
    let measured_ping = crate::mihomo::ping_server_url(_http, _cfg, target_server, 4000, Some("https://cp.cloudflare.com")).await;
    if measured_ping > 0 {
        first_byte_ms = measured_ping as u32;
    }

    // 5. Тестовые зеркала для скачивания чанка (5-10 МБ)
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
                restore_groups_snapshot(_http, _cfg, &group_snapshot).await;
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
                restore_groups_snapshot(_http, _cfg, &group_snapshot).await;
            }
            return Err(format!("Ошибка инициализации HTTP клиента: {}", e));
        }
    };

    // 6. Измерение ТОЛЬКО интервала передачи данных (DIAG-01)
    let mut total_bytes = 0u64;
    let mut transfer_duration_secs = 0.0f64;
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

                let transfer_start = Instant::now();
                let mut chunk_bytes = 0u64;
                while let Ok(Some(chunk)) = resp.chunk().await {
                    chunk_bytes += chunk.len() as u64;
                    // Если скачали больше 5 МБ или прошло больше 8 секунд — завершаем замер
                    if chunk_bytes >= 5 * 1024 * 1024 || transfer_start.elapsed().as_secs() >= 8 {
                        break;
                    }
                }
                let elapsed = transfer_start.elapsed().as_secs_f64().max(0.001);
                if chunk_bytes > 100_000 {
                    total_bytes = chunk_bytes;
                    transfer_duration_secs = elapsed;
                    success = true;
                    break;
                }
            }
            Err(e) => {
                log_w!("Ошибка скачивания с {}: {}", url, e);
            }
        }
    }

    // 7. Восстанавливаем исходную структуру групп выбора сразу после замера
    if need_restore {
        restore_groups_snapshot(_http, _cfg, &group_snapshot).await;
    }

    if !success || total_bytes == 0 {
        return Err("Не удалось выполнить тест скорости: тестовые узлы недоступны".to_string());
    }

    // Время замера включает строго интервал передачи данных, без накладных расходов на переключение и откат
    let speed_mbps = calculate_speed(total_bytes, transfer_duration_secs);

    log_i!(
        "Тест скорости для '{}' завершён: {} Мбит/с ({} байт за {:.2} с, отклик {} мс)",
        target_server,
        speed_mbps,
        total_bytes,
        transfer_duration_secs,
        first_byte_ms
    );

    Ok(SpeedtestResponse {
        server_id: target_server.to_string(),
        latency_ms: first_byte_ms,
        speed_mbps,
        bytes_downloaded: total_bytes,
        duration_secs: (transfer_duration_secs * 100.0).round() / 100.0,
        verified: true,
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
