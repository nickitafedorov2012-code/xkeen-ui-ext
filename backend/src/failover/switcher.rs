use std::time::{Instant, Duration};
use crate::AppState;
use super::*;

/// Проверяет, является ли узел сервером белых списков:
/// 1) По ID из `cfg.whitelist_servers`
/// 2) По провайдеру/подписке из `cfg.whitelist_providers`
/// 3) По тегам в названии `[БС]`, `(БС)`, `[WL]`, `(WL)`, `whitelist` (case-insensitive).
pub fn is_whitelist_server(node_name: &str, node_id: &str, cfg: &crate::config::AppConfig) -> bool {
    is_whitelist_server_full(node_name, node_id, None, cfg)
}

pub fn provider_matches_whitelist(provider: &str, cfg: &crate::config::AppConfig) -> bool {
    crate::whitelist_tags::provider_matches_whitelist(provider, cfg)
}

pub fn is_whitelist_server_full(
    node_name: &str,
    node_id: &str,
    provider: Option<&str>,
    cfg: &crate::config::AppConfig,
) -> bool {
    crate::whitelist_tags::is_whitelist_server_full(node_name, node_id, provider, cfg)
}

/// Баг 4: можно ли после перезагрузки вернуть игровой режим, приостановленный ради БС.
/// Только если Mihomo ответил, активный узел известен и он не БС (с учётом подписок).
pub fn startup_gaming_restore_allowed(
    mihomo_answered: bool,
    active: Option<&crate::mihomo::Server>,
    cfg: &crate::config::AppConfig,
) -> bool {
    if !cfg.whitelist.gaming_suspended || !mihomo_answered {
        return false;
    }
    match active {
        Some(s) => !is_whitelist_server_full(&s.name, &s.id, s.provider.as_deref(), cfg),
        None => false,
    }
}

/// Сообщение при попытке включить игровой режим на узле БС (баг 5).
pub const GAMING_BLOCKED_BY_BS: &str =
    "Сейчас активен сервер белых списков (БС) — игровой режим работает только на обычном сервере";

/// Баг 5: включение игрового режима запрещено, пока активен узел БС («строго два канала»).
/// Известен активный узел — решаем по нему (с учётом подписок); неизвестен — по флагу приостановки.
pub fn gaming_enable_blocked_by_bs(cfg: &crate::config::AppConfig, active: Option<&crate::mihomo::Server>) -> bool {
    match active {
        Some(s) => is_whitelist_server_full(&s.name, &s.id, s.provider.as_deref(), cfg),
        None => cfg.whitelist.gaming_suspended,
    }
}

/// Синхронизация селекторной группы '🛡️ Whitelist' в ядре Mihomo при переключении узла.
pub async fn sync_whitelist_on_switch(
    http: &reqwest::Client,
    cfg: &crate::config::AppConfig,
    node_id: &str,
    node_name: &str,
) {
    if !cfg.whitelist.enabled {
        return;
    }

    let is_auto = is_whitelist_server(node_name, node_id, cfg);
    let should_follow = cfg.whitelist.follow_failover
        || cfg.whitelist.target_server.as_deref() == Some("follow_failover")
        || cfg.whitelist.target_server.as_deref().map_or(true, |s| s.trim().is_empty());

    // Дедупликация: не спамить Mihomo API и логи, если тот же узел уже синхронизирован недавно (< 2 сек)
    {
        let mut last_sync = LAST_WHITELIST_SYNC.lock().await;
        if let Some((ref last_id, ref last_time)) = *last_sync {
            if last_id == node_id && last_time.elapsed() < Duration::from_secs(2) {
                return;
            }
        }
        *last_sync = Some((node_id.to_string(), Instant::now()));
    }

    if should_follow && !node_id.is_empty() {
        // Только селектор: сброс сокетов уже решил основной switch_server/_graceful.
        // switch_group здесь рвал ВСЕ соединения LAN и ломал мягкое переключение.
        let _ = crate::mihomo::switch_selector(http, cfg, crate::routing::WHITELIST_GROUP_NAME, node_id).await;
    }

    crate::log_i!(
        "[FAILOVER] Whitelist mode active server switched to: {} (auto-detected: {})",
        node_name,
        is_auto
    );
}

/// Переход режима БС после отметки/снятия отметки подписки или сервера (Баг A-11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhitelistTransition {
    Enable,
    Disable,
}

/// Чистая функция расчёта автоматического перехода БС после отметки активного узла/подписки (Баг A-11).
pub fn whitelist_transition_after_marking(
    cfg: &crate::config::AppConfig,
    active_server: Option<(&str, &str, Option<&str>)>,
) -> Option<WhitelistTransition> {
    if !cfg.whitelist.auto_by_failover {
        return None;
    }
    let (node_id, node_name, provider) = active_server?;
    let is_bs = is_whitelist_server_full(node_name, node_id, provider, cfg);
    if is_bs && !cfg.whitelist.enabled {
        return Some(WhitelistTransition::Enable);
    }
    if !is_bs
        && cfg.whitelist.enabled
        && cfg.whitelist.enabled_by == crate::config::WhitelistEnabledBy::Failover
    {
        return Some(WhitelistTransition::Disable);
    }
    None
}

/// Изменение флагов при переходе failover на узел [БС] или обратно (Zero-Touch, баг 108).
/// Возвращает (что-то изменилось, пояснение для журнала).
pub fn apply_whitelist_transition_flags(cfg: &mut crate::config::AppConfig, is_bs: bool) -> (bool, String) {
    let mut changed = false;
    let mut gaming_log = String::new();
    if is_bs {
        if !cfg.whitelist.enabled {
            cfg.whitelist.enabled = true;
            cfg.whitelist.enabled_by = crate::config::WhitelistEnabledBy::Failover;
            changed = true;
        }
        if cfg.gaming.enabled {
            cfg.gaming.enabled = false;
            cfg.whitelist.gaming_suspended = true;
            changed = true;
            gaming_log = " (Игровой режим приостановлен во избежание конфликтов сокетов)".to_string();
        }
    } else {
        // Баг 107: failover выключает БС только если он был включён самим failover (Failover).
        // Если пользователь включил БС вручную (Manual), failover не должен сбрасывать режим.
        if cfg.whitelist.enabled && cfg.whitelist.enabled_by == crate::config::WhitelistEnabledBy::Failover {
            cfg.whitelist.enabled = false;
            changed = true;
        }
        if cfg.whitelist.gaming_suspended {
            cfg.gaming.enabled = true;
            cfg.whitelist.gaming_suspended = false;
            changed = true;
            gaming_log = " (Игровой режим автоматически восстановлен)".to_string();
        }
    }
    (changed, gaming_log)
}

/// Комплексная обработка перехода на сервер с учетом режима Белых списков (Zero-Touch):
/// 1. Если включен auto_by_failover:
///    - Если узел помечен как [БС]: автоматически активирует whitelist.enabled,
///      отключает gaming.enabled (выставляя gaming_suspended = true),
///      синхронизирует селекторную группу 🛡️ Whitelist на новый узел.
///    - Если узел обычный: деактивирует whitelist.enabled,
///      восстанавливает gaming.enabled (если gaming_suspended == true).
/// 2. Применяет правила маршрутизации и сохраняет состояние.
pub async fn handle_whitelist_transition(
    state: &AppState,
    node_id: &str,
    node_name: &str,
    provider: Option<&str>,
) {
    let cfg = state.config.read().await.clone();
    let is_bs = is_whitelist_server_full(node_name, node_id, provider, &cfg);

    if is_bs {
        // Сброс счетчика эпизода белых списков при переходе на сервер БС (Tier 2)
        reset_whitelist_episode_bytes();
    }

    if cfg.whitelist.auto_by_failover {
        // Баг 108: флаги БС/гейминга, config.yaml и reload — одна транзакция. Если ядро не приняло конфиг,
        // ничего не сохраняется и iptables не трогаются: config.json, правила и фаервол не расходятся.
        let mut probe = (**state.config.read().await).clone();
        let (cfg_changed, gaming_log) = apply_whitelist_transition_flags(&mut probe, is_bs);
        if cfg_changed {
            match crate::api::commit_config_with_routing(state, |c| {
                let _ = apply_whitelist_transition_flags(c, is_bs);
            })
            .await
            {
                Ok((cur, applied_yaml)) => {
                    if is_bs {
                        crate::log_i!(
                            "[FAILOVER] 🛡️ Авто-активация режима Белых списков (узел '{}' помечен как [БС]){}",
                            node_name,
                            gaming_log
                        );
                    } else {
                        crate::log_i!(
                            "[FAILOVER] ⚪ Деактивация режима Белых списков (переход на обычный узел '{}'){}",
                            node_name,
                            gaming_log
                        );
                    }
                    // Синхронизируем режимы фаервола и цепочку XKEEN_BS в iptables
                    let ports = crate::routing::redir_ports(&applied_yaml);
                    let (redir_port, tproxy_port) = (ports.redir, ports.tproxy);
                    let dns_port = crate::api::resolve_active_dns_port(&cur);
                    crate::api::sync_full_tunnel_modes(&cur, redir_port, tproxy_port, dns_port).await;
                }
                Err(e) => {
                    crate::log_e!("[FAILOVER] Режим Белых списков не переключён — ядро не приняло конфигурацию: {e}");
                }
            }
        }

        if is_bs {
            let _ = crate::mihomo::switch_selector(&state.http, &cfg, crate::routing::WHITELIST_GROUP_NAME, node_id).await;
        }
    } else if cfg.whitelist.enabled {
        // Селектор без сброса сокетов: решение о сбросе принимает вызывающий switch_server(_graceful)
        let _ = crate::mihomo::switch_selector(&state.http, &cfg, crate::routing::WHITELIST_GROUP_NAME, node_id).await;
    }

    // FO-WL-03: Instant telemetry channel sync: актуализируем статус телеметрии сразу,
    // чтобы график мгновенно переключил второй канал между фиолетовым БС и синим VPS без ожидания 15-секундного тика
    let current_cfg_now = state.config.read().await.clone();
    let is_wl_now = crate::api::compute_whitelist_mode_active(&current_cfg_now, node_id, node_name, provider);
    crate::traffic::set_whitelist_active(is_wl_now);
}
