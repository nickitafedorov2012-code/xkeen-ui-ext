use std::path::Path;
use tokio::time::{sleep, Duration};

use crate::{log_i, log_w, mihomo, override_sync, routing, AppState};

/// Фоновый сторожевой процесс (watchdog) для защиты конфигурации config.yaml.
/// Если служба XKeen перезапускается (S05xkeen restart или автообновление по расписанию),
/// XKeen перегенерирует config.yaml, затирая блоки AUTO-DEVICE и AUTO-FORCE.
/// Watchdog отслеживает изменения и мгновенно восстанавливает правила маршрутизации.
pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        // Начальная пауза перед запуском монитора
        sleep(Duration::from_secs(10)).await;

        let mut last_mtime: Option<std::time::SystemTime> = None;
        let mut last_len: usize = 0;

        loop {
            sleep(Duration::from_secs(4)).await;

            let (config_path_str, force_domains, device_routing, ignore_servers, device_domains) = {
                let cfg = state.config.read().await;
                (
                    cfg.mihomo.config_path.clone(),
                    cfg.force_domains.clone(),
                    cfg.device_routing.clone(),
                    cfg.ignore_servers.clone(),
                    cfg.device_domain_rules.clone(),
                )
            };

            let path = Path::new(&config_path_str);
            if !path.exists() {
                continue;
            }

            let meta = match std::fs::metadata(path) {
                Ok(m) => m,
                Err(_) => continue,
            };

            let mtime = meta.modified().ok();
            let len = meta.len() as usize;

            // Если размер или дата модификации изменились
            let file_changed = match (mtime, last_mtime) {
                (Some(curr), Some(prev)) => curr != prev || len != last_len,
                (Some(_), None) => true,
                _ => false,
            };

            if file_changed {
                last_mtime = mtime;
                last_len = len;

                // Читаем содержимое и проверяем наличие сохраненных маркер-блоков
                if let Ok(content) = std::fs::read_to_string(path) {
                    let needs_device = !device_routing.is_empty() || !device_domains.is_empty();
                    let needs_force = !force_domains.is_empty();
                    let needs_ignore = !ignore_servers.is_empty();

                    let missing_device = needs_device && !content.contains("AUTO-DEVICE");
                    let missing_force = needs_force && !content.contains("AUTO-FORCE");
                    let missing_ignore = needs_ignore && !content.contains("AUTO-IGNORE");

                    if missing_device || missing_force || missing_ignore {
                        log_w!("[WATCHDOG] Обнаружена перезапись config.yaml (рестарт XKeen)! Восстановление маршрутизации...");

                        let _guard = state.routing_lock.lock().await;
                        let cfg = state.config.read().await.clone();

                        // Повторное применение маршрутизации
                        let raw_yaml = match std::fs::read_to_string(path) {
                            Ok(c) => c,
                            Err(e) => {
                                log_w!("[WATCHDOG] Ошибка чтения config.yaml: {}", e);
                                continue;
                            }
                        };

                        let (new_yaml, _applied) = routing::apply_routing(&raw_yaml, &cfg);
                        let tmp = format!("{}.tmp", path.display());
                        if let Err(e) = std::fs::write(&tmp, &new_yaml).and_then(|_| std::fs::rename(&tmp, path)) {
                            log_w!("[WATCHDOG] Ошибка записи config.yaml: {}", e);
                            continue;
                        }

                        // Перезагрузка ядра Mihomo
                        if let Err(e) = mihomo::reload_config(&state.http, &cfg).await {
                            log_w!("[WATCHDOG] Ошибка перезагрузки Mihomo: {}", e);
                        } else {
                            log_i!("[WATCHDOG] ✓ Правила маршрутизации успешно восстановлены и применены в ядре");
                        }

                        // Синхронизация ipset geo_override
                        if !cfg.force_domains.is_empty() {
                            let auto_cdns = crate::cdn_discovery::discover_all_cdns(&cfg.force_domains, &cfg.mihomo_proxy_url()).await;
                            let mut all_domains = cfg.force_domains.clone();
                            all_domains.extend(auto_cdns);
                            let _ = override_sync::sync_geo_override(&all_domains).await;
                        }

                        // Обновляем зафиксированные метаданные
                        if let Ok(new_meta) = std::fs::metadata(path) {
                            last_mtime = new_meta.modified().ok();
                            last_len = new_meta.len() as usize;
                        }
                    }
                }
            }
        }
    });
}
