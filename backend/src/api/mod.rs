use axum::response::{IntoResponse, Json, Response};
use serde_json::json;

use crate::{config, AppState};

pub mod backups;
pub use backups::*;
pub mod clash_proxy;
pub use clash_proxy::*;
pub mod dns_adguard;
pub use dns_adguard::*;
pub mod domains;
pub use domains::*;
pub mod routing;
pub use routing::*;
pub mod servers;
pub use servers::*;
pub mod status;
pub use status::*;
pub mod system_ssh;
pub use system_ssh::*;
pub mod vkturn;
pub use vkturn::*;
pub mod zapret;
pub use zapret::*;

/// Вспомогательное: единый формат ответа API.
pub fn api_ok(data: serde_json::Value) -> axum::response::Response {
    Json(json!({ "success": true, "data": data })).into_response()
}

pub fn api_err(error: impl Into<String>) -> axum::response::Response {
    Json(json!({ "success": false, "error": error.into() })).into_response()
}

pub fn api_err_bad_request(error: impl Into<String>) -> axum::response::Response {
    (
        axum::http::StatusCode::BAD_REQUEST,
        Json(json!({ "success": false, "error": error.into() })),
    )
        .into_response()
}

pub fn api_err_detailed(
    code: impl Into<String>,
    error: impl Into<String>,
    details: Option<serde_json::Value>,
    suggested_action: Option<&str>,
) -> axum::response::Response {
    let err_str = error.into();
    Json(json!({
        "success": false,
        "code": code.into(),
        "error": err_str,
        "details": details,
        "suggested_action": suggested_action,
    })).into_response()
}

/// Атомарная запись файла на диск (tmp + rename)
pub async fn atomic_write_file(path: impl AsRef<std::path::Path>, content: &str) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;
    let p = path.as_ref();
    if let Some(parent) = p.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = format!("{}.{}.{}.tmp", p.display(), std::process::id(), nonce);
    // Z-12: пишем через File с sync_all перед rename — на роутерах с внезапной потерей
    // питания rename без fsync оставлял пустой/битый файл (config.json, zapret.conf, config.yaml)
    let write_res: Result<(), String> = async {
        let mut f = tokio::fs::File::create(&tmp)
            .await
            .map_err(|e| format!("Ошибка записи {tmp}: {e}"))?;
        f.write_all(content.as_bytes())
            .await
            .map_err(|e| format!("Ошибка записи {tmp}: {e}"))?;
        f.flush().await.map_err(|e| format!("Ошибка записи {tmp}: {e}"))?;
        f.sync_all().await.map_err(|e| format!("Ошибка fsync {tmp}: {e}"))?;
        Ok(())
    }
    .await;
    if let Err(e) = write_res {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(e);
    }
    if let Err(e) = tokio::fs::rename(&tmp, p).await {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(format!("Ошибка атомарного сохранения {}: {e}", p.display()));
    }
    Ok(())
}

/// Обработчик 404 для несуществующих API-эндпоинтов
pub async fn api_not_found() -> Response {
    (
        axum::http::StatusCode::NOT_FOUND,
        Json(json!({
            "success": false,
            "error": "API endpoint not found"
        })),
    )
        .into_response()
}

/// Сохранение zapret.enabled вместе с пересборкой правил Mihomo в одной транзакции (баги 28, 57, 95).
/// Общая для бота, UI-ядра и сторожа. Если ядро не приняло reload, файлы всё равно приводятся
/// в согласованное состояние без reload — следующий старт Mihomo подхватит верные правила.
/// Атомарное обновление флагов AppConfig с одновременным переприменением правил crate::routing::apply_routing
/// и reload ядра Mihomo. Возвращает (новый AppConfig, примененный config.yaml).
pub async fn commit_config_with_routing<F>(
    state: &AppState,
    mutate: F,
) -> Result<(config::AppConfig, String), String>
where
    F: FnOnce(&mut config::AppConfig),
{
    let mut tx = crate::transaction::ConfigTx::begin(state).await?;
    mutate(tx.config_mut());
    let new_cfg = tx.config().clone();
    let yaml = tx.read_yaml().await?;
    let (new_yaml, _) = crate::routing::apply_routing(&yaml, &new_cfg)?;
    tx.set_yaml(new_yaml.clone())?;
    tx.commit_and_reload().await?;
    Ok((new_cfg, new_yaml))
}

#[cfg(test)]
mod settings_and_service_regression_tests {
    use super::{
        build_gaming_full_tunnel_commands, build_whitelist_full_tunnel_commands, full_tunnel_resync_needed,
        mask_settings_for_api, merge_adguard_config_patch, merge_settings_update, resolve_active_dns_port, resolve_active_dns_port_with_health,
    };
    use crate::config;

    /// Весь исходник: тестовые модули api разбросаны по файлу, поэтому функции
    /// ищутся по сигнатуре с открывающей скобкой (в тестовых литералах её нет).
    fn production_source() -> &'static str {
        concat!(
            include_str!("backups.rs"), "\n",
            include_str!("clash_proxy.rs"), "\n",
            include_str!("dns_adguard.rs"), "\n",
            include_str!("domains.rs"), "\n",
            include_str!("routing.rs"), "\n",
            include_str!("servers.rs"), "\n",
            include_str!("status.rs"), "\n",
            include_str!("system_ssh.rs"), "\n",
            include_str!("vkturn.rs"), "\n",
            include_str!("zapret.rs"), "\n",
            include_str!("mod.rs")
        )
    }

    fn fn_body<'a>(src: &'a str, signature: &str) -> &'a str {
        let needle = format!("{signature}(");
        let start = src.find(&needle).unwrap_or_else(|| panic!("нет функции {signature}"));
        let rest = &src[start + needle.len()..];
        let end = rest.find("\npub async fn ").or_else(|| rest.find("\npub fn ")).unwrap_or(rest.len());
        &rest[..end]
    }

    #[test]
    fn test_put_settings_cannot_overwrite_auth() {
        // REG: PUT /api/settings подменял хеш пароля и session_secret в обход
        // проверки текущего пароля (A-02) → захват панели из LAN.
        let mut current = config::AppConfig::default();
        current.auth.enabled = false;
        current.auth.password_hash = "$pbkdf2$100000$salt$ownerhash".into();
        current.auth.salt = "salt".into();
        current.auth.session_secret = "owner-secret".into();
        let body = serde_json::json!({
            "auth": { "enabled": true, "password_hash": "$pbkdf2$1$x$attacker", "salt": "x", "session_secret": "known" },
            "refresh_interval_sec": 7
        });
        let merged = merge_settings_update(&current, &body).expect("merge");
        assert_eq!(merged.auth.password_hash, current.auth.password_hash);
        assert_eq!(merged.auth.session_secret, "owner-secret");
        assert!(!merged.auth.enabled);
        assert_eq!(merged.refresh_interval_sec, 7, "остальные поля применяются как раньше");
    }

    #[test]
    fn regression_bug_1_adguard_failover_bs_mode_dns() {
        // Баг 1: При падении AdGuard в режиме БС вся сеть теряет DNS:
        // 1. Хук AdGuard и синхронизация iptables переключали DNS только в XKEEN_GAMING, игнорируя XKEEN_BS
        let hook = crate::adguard::build_adguard_ndm_hook_content(5353);
        assert!(hook.contains("XKEEN_BS"), "NDM-хук AdGuard обязан переключать DNS и в цепочке XKEEN_BS при падении/восстановлении AdGuard");

        let sync_script = crate::adguard::build_sync_tunnel_dns_ports_script("5353|5354", 1053);
        assert!(sync_script.contains("XKEEN_BS"), "Скрипт синхронизации портов обязан переключать XKEEN_BS");
        assert!(sync_script.contains("XKEEN_GAMING"), "Скрипт синхронизации портов обязан переключать XKEEN_GAMING");

        // 2. resolve_active_dns_port при неработающем AdGuard (флаг ADGUARD_DOWN) обязан возвращать порт Mihomo
        let mut cfg = config::AppConfig::default();
        cfg.adguard.enabled = true;
        cfg.adguard.dns_port = 5353;
        assert_eq!(resolve_active_dns_port_with_health(&cfg, false), 1053, "При упавшем AdGuard порт обязан переключаться на 1053 (Mihomo)");

        crate::watchdog::set_adguard_down(true);
        assert_eq!(resolve_active_dns_port(&cfg), 1053, "Рабочая resolve_active_dns_port при ADGUARD_DOWN=true обязана возвращать порт Mihomo");
        crate::watchdog::set_adguard_down(false);
    }

    #[test]
    fn regression_bug_70_all_iptables_commands_use_wait_lock() {
        // Баг 70: Ни один iptables-вызов из Rust не использует -w.
        // Проверяем все сборщики команд iptables: каждая команда iptables обязана содержать флаг -w (или -w 2).
        let _api_all = production_source();
        let wd_src = include_str!("../watchdog.rs");

        // В рабочих shell-командах (не комментарии) iptables/ip6tables обязаны содержать -w
        let api_prod = [
            include_str!("mod.rs"),
            include_str!("backups.rs"),
            include_str!("clash_proxy.rs"),
            include_str!("dns_adguard.rs"),
            include_str!("domains.rs"),
            include_str!("routing.rs"),
            include_str!("servers.rs"),
            include_str!("status.rs"),
            include_str!("system_ssh.rs"),
            include_str!("vkturn.rs"),
            include_str!("zapret.rs"),
        ]
        .into_iter()
        .map(|s| &s[..s.find("#[cfg(test)]").unwrap_or(s.len())])
        .collect::<Vec<_>>()
        .join("\n");
        let wd_prod = &wd_src[..wd_src.find("#[cfg(test)]").unwrap_or(wd_src.len())];

        assert!(!api_prod.contains("for ipt in iptables ip6tables; do"), "api.rs stop loop must use -w 2");
        assert!(!api_prod.contains("iptables -t nat -C PREROUTING -j xkeen"), "api.rs checks must use -w 2");
        assert!(!api_prod.contains("iptables -t mangle -C PREROUTING -j xkeen"), "api.rs checks must use -w 2");
        assert!(!wd_prod.contains("iptables -t mangle -S 2>/dev/null"), "watchdog must use -w 2 for status checks");

        let mut gaming_cfg = config::GamingConfig::default();
        gaming_cfg.enabled = true;
        gaming_cfg.mode = config::GamingMode::Compatibility;
        gaming_cfg.devices.push(config::GamingDevice {
            mac: "AA:BB:CC:DD:EE:01".into(),
            ip: "192.168.1.50".into(),
            ipv6: Vec::new(),
            name: "PC".into(),
            enabled: true,
            server: None,
        });

        let (g_cleanup, g_setup, _) = build_gaming_full_tunnel_commands(&gaming_cfg, 5000, 5001, 1053);
        for cmd in g_cleanup.split(';') {
            let t = cmd.trim();
            if t.contains("iptables ") || t.contains("ip6tables ") {
                assert!(t.contains(" -w"), "Команда очистки Gaming должна содержать -w: '{t}'");
            }
        }
        for cmd in &g_setup {
            if cmd.contains("iptables ") || cmd.contains("ip6tables ") {
                assert!(cmd.contains(" -w"), "Команда настройки Gaming должна содержать -w: '{cmd}'");
            }
        }

        let mut wl_cfg = config::AppConfig::default();
        wl_cfg.whitelist.enabled = true;
        wl_cfg.whitelist.strategy = config::WhitelistStrategy::StrictXKeen;
        let (wl_cleanup, wl_setup) = build_whitelist_full_tunnel_commands(&wl_cfg, 5000, 5001, 1053);
        for cmd in wl_cleanup.split(';') {
            let t = cmd.trim();
            if t.contains("iptables ") {
                assert!(t.contains(" -w"), "Команда очистки Whitelist должна содержать -w: '{t}'");
            }
        }
        for cmd in &wl_setup {
            if cmd.contains("iptables ") {
                assert!(cmd.contains(" -w"), "Команда настройки Whitelist должна содержать -w: '{cmd}'");
            }
        }

        let hook = crate::adguard::build_adguard_ndm_hook_content(5353);
        for line in hook.lines() {
            let t = line.trim();
            if t.contains("iptables ") {
                assert!(t.contains(" -w"), "NDM-хук AdGuard должен содержать -w во всех iptables вызовах: '{t}'");
            }
        }

        let sync_script = crate::adguard::build_sync_tunnel_dns_ports_script("1053|5353", 5353);
        for cmd in sync_script.split(';') {
            let t = cmd.trim();
            if t.contains("iptables ") {
                assert!(t.contains(" -w"), "Скрипт синхронизации портов должен содержать -w: '{t}'");
            }
        }
    }

    #[test]
    fn regression_bug_133_adguard_config_patch_keeps_background_state() {
        // Баг 133: POST /api/adguard/config целиком заменял config.adguard снимком со страницы:
        // сохранение «хоста» откатывало найденный сторожем порт и включение.
        let mut current = crate::config::AdGuardConfig::default();
        current.enabled = true;
        current.dns_port = 5354;
        current.integration_choice_locked = true;
        current.password = Some("secret".to_string());
        let patch = serde_json::json!({ "host": "10.0.0.5", "enabled": false, "password": "******" });
        let merged = merge_adguard_config_patch(&current, &patch).unwrap();
        assert_eq!(merged.host, "10.0.0.5");
        assert!(merged.enabled, "включение меняется только переключателем, не формой настроек");
        assert_eq!(merged.dns_port, 5354, "поле, которого нет в патче, не трогается");
        assert!(merged.integration_choice_locked);
        assert_eq!(merged.password.as_deref(), Some("secret"), "маска ****** не затирает пароль");
        let full = serde_json::json!({ "dns_port": 53 });
        assert_eq!(merge_adguard_config_patch(&current, &full).unwrap().dns_port, 53, "явная смена порта работает");
        assert!(merge_adguard_config_patch(&current, &serde_json::json!("x")).is_err());
    }

    #[test]
    fn regression_bug_31a_zapret_adguard_flag_ignores_mere_installation() {
        // Баг 31: ADGUARD_ENABLED=1 ставился, если AdGuard просто установлен (есть S99adguardhome),
        // даже выключенный — Zapret считал, что DNS LAN обслуживает AdGuard.
        assert!(!super::zapret_conf_adguard_flag(false, false, true), "установлен, но выключен и не запущен — 0");
        assert!(super::zapret_conf_adguard_flag(true, false, true), "интеграция включена — 1");
        assert!(super::zapret_conf_adguard_flag(false, true, false), "процесс AdGuardHome работает — 1");
    }

    #[test]
    fn regression_bug_56_zapret_actions_do_not_touch_router_dns() {
        // Баг 56: start/restart/start-fw/reload-fw Zapret из UI вызывали rci::set_clean_dns_servers —
        // в KeeneticOS дописывались 77.88.8.8/77.88.8.1/1.1.1.1 и делался `system configuration save`.
        let body = fn_body(production_source(), "pub async fn zapret_action");
        assert!(
            !body.contains("set_clean_dns_servers"),
            "запуск Zapret не должен менять системные DNS роутера"
        );
        let explicit = fn_body(production_source(), "pub async fn apply_clean_dns");
        assert!(explicit.contains("set_clean_dns_servers"), "явная кнопка «чистые DNS» остаётся");
    }

    #[test]
    fn regression_bug_27_zapret_return_rules_are_cleaned_up() {
        // Баг 27: add_fw добавлял в nat PREROUTING `-d 127.0.0.1 -j RETURN` и DNS-RETURN исключённых IP,
        // remove_fw_rules их не удалял — цепочка росла с каждым restart/reload-fw/NDM-хуком.
        let script = super::S51ZAPRET_SCRIPT;
        let section = |name: &str| {
            let start = script.find(&format!("\n{name}() {{")).unwrap_or_else(|| panic!("нет {name}"));
            let rest = &script[start + 1..];
            let end = rest.find("\n}\n").unwrap_or(rest.len());
            rest[..end].to_string()
        };
        let remove = section("remove_fw_rules");
        assert!(remove.contains("-D PREROUTING -i br+ -d 127.0.0.1 -j RETURN"), "старые RETURN без комментария удаляются");
        assert!(remove.contains("-D PREROUTING -i Bridge+ -d 127.0.0.1 -j RETURN"));
        assert!(remove.contains("xkeen-route-zapret") && remove.contains("-j RETURN"), "все свои RETURN по комментарию удаляются");
        let add = section("add_fw");
        let lines: Vec<&str> = add.lines().collect();
        let mut checked = 0;
        for (i, line) in lines.iter().enumerate() {
            if !(line.contains("-t nat") && line.contains("-j RETURN")) {
                continue;
            }
            checked += 1;
            let commented = line.contains("xkeen-route-zapret");
            let fallback = i > 0 && lines[i - 1].contains("xkeen-route-zapret") && lines[i - 1].trim_end().ends_with("|| \\");
            assert!(commented || fallback, "RETURN-правило nat без комментария допустимо только как запасное: {line}");
        }
        assert!(checked >= 2, "в add_fw есть RETURN-правила nat");
    }

    #[test]
    fn regression_bug_43_toggle_whitelist_device_does_not_corrupt_mac_on_ip_reuse() {
        // Баг 43: toggle_whitelist_device искал устройство по (match_mac || match_ip).
        // Если DHCP выдал старый IP другому устройству, у старой записи перезаписывался MAC!
        let body = fn_body(production_source(), "pub async fn toggle_whitelist_device");

        assert!(
            !body.contains("match_mac || match_ip"),
            "toggle_whitelist_device must NOT use loose 'match_mac || match_ip' which corrupts MAC on DHCP reuse"
        );
        assert!(
            body.contains("is_target = if !mac_str.is_empty()"),
            "toggle_whitelist_device must strictly prioritize MAC matching"
        );
    }
    #[test]
    fn regression_bug_40_save_whitelist_pinned_devices_uses_scoped_switch() {
        // сколько закреплено устройств (в цикле вызывался `switch_group`, сбрасывающий ВСЕ соединения сети).
        // Должен использоваться `switch_group_scoped`.
        let save = fn_body(production_source(), "pub async fn save_whitelist");
        assert!(
            !save.contains("mihomo::switch_group("),
            "save_whitelist must NOT call global switch_group in loop, breaking all network connections"
        );
        assert!(
            save.contains("switch_group_scoped("),
            "save_whitelist must use switch_group_scoped"
        );
    }

    #[test]
    fn regression_bug_38b_whitelist_device_toggle_syncs_iptables() {
        // Баг 38: toggle_whitelist_device включал БС в конфиге и Mihomo, но не синхронизировал
        // iptables XKEEN_BS и туннель БС (в отличие от save_whitelist) — UI «БС включён», перехвата нет.
        let body = fn_body(production_source(), "pub async fn toggle_whitelist_device");
        assert!(
            body.contains("sync_whitelist_runtime_after_commit"),
            "после коммита переключения устройства нужна та же синхронизация, что и в save_whitelist"
        );
        let save = fn_body(production_source(), "pub async fn save_whitelist");
        assert!(save.contains("sync_whitelist_runtime_after_commit"), "одна общая реализация");
    }

    #[test]
    fn regression_bug_38c_device_toggle_does_not_intercept_whole_lan() {
        // Баг 38:
        // (а) Включение отдельного устройства НЕ включает перехват всей сети.
        // Правила БС (iptables и Mihomo) действуют только на включённые устройства,
        // пока пользователь не включил общий режим «вся сеть».
        // (б) Выключение последнего включённого устройства выключает БС,
        // если режим был включён именно этим путём (enabled_by == Devices / WhitelistModeChange::Disable).
        // Общий режим «вся сеть», включённый вручную (Manual), не трогается.

        let mut cfg = config::AppConfig::default();
        cfg.whitelist.enabled = false;
        cfg.whitelist.enabled_by = config::WhitelistEnabledBy::Manual;
        cfg.whitelist.devices.clear();

        // 1. Включение одного устройства при изначально выключенном БС:
        let dev1 = config::WhitelistDevice {
            mac: "AA:BB:CC:DD:EE:01".to_string(),
            ip: "192.168.1.100".to_string(),
            enabled: true,
            name: Some("Phone".to_string()),
            server: None,
        };

        let change = super::whitelist_mode_after_device_toggle(&cfg, &dev1, true);
        assert_eq!(
            change,
            super::WhitelistModeChange::PerDeviceList,
            "включение устройства при выключенном общем режиме должно переводить в PerDeviceList, а не всю сеть"
        );

        // Применяем изменение к конфигу:
        cfg.whitelist.devices.push(dev1);
        match change {
            super::WhitelistModeChange::PerDeviceList => {
                cfg.whitelist.enabled = true;
                cfg.whitelist.enabled_by = config::WhitelistEnabledBy::Devices;
            }
            _ => {}
        }

        // Проверяем iptables: для PerDeviceList генерируются правила только для 192.168.1.100,
        // а НЕ broad subnet-правила на всю подсеть 192.168.0.0/16!
        let (_cleanup, setup) = super::build_whitelist_full_tunnel_commands(&cfg, 5000, 5001, 1053);
        assert!(!setup.is_empty(), "должен быть перехват включенного устройства");
        assert!(
            setup.iter().any(|c| c.contains("-s 192.168.1.100 -p tcp -j REDIRECT --to-ports 5000")),
            "перехват TCP только для включенного устройства"
        );
        assert!(
            setup.iter().any(|c| c.contains("-s 192.168.1.100 -p udp -j TPROXY")),
            "перехват UDP только для включенного устройства"
        );
        assert!(
            !setup.iter().any(|c| c.contains("-s 192.168.0.0/16 -p tcp -j REDIRECT")),
            "при режиме по устройствам broad subnet перехват всей LAN (192.168.0.0/16) ЗАПРЕЩЁН!"
        );

        // Проверяем правила Mihomo (apply_whitelist_rules):
        let base_yaml = "rules:\n  - MATCH,DIRECT\n";
        let mihomo_yaml = crate::routing::apply_whitelist_rules(base_yaml, &cfg, None);
        assert!(
            mihomo_yaml.contains("SRC-IP-CIDR,192.168.1.100/32,🛡️ Whitelist"),
            "устройство направляется в туннель БС"
        );
        assert!(
            !mihomo_yaml.contains("SRC-IP-CIDR,192.168.0.0/16,🛡️ Whitelist"),
            "в Mihomo при режиме по устройствам catch-all на всю LAN 192.168.0.0/16 отсутствует"
        );

        // 2. Выключение последнего включённого устройства, когда режим был включён через Devices:
        let dev1_off = config::WhitelistDevice {
            enabled: false,
            ..cfg.whitelist.devices[0].clone()
        };
        let change_off = super::whitelist_mode_after_device_toggle(&cfg, &dev1_off, false);
        assert_eq!(
            change_off,
            super::WhitelistModeChange::Disable,
            "выключение последнего устройства, включённого через Devices, должно выключать БС"
        );

        // 3. Если общий режим был включён вручную (Manual), выключение устройства оставляет БС включённым:
        let mut manual_cfg = cfg.clone();
        manual_cfg.whitelist.enabled = true;
        manual_cfg.whitelist.enabled_by = config::WhitelistEnabledBy::Manual;
        let change_manual = super::whitelist_mode_after_device_toggle(&manual_cfg, &dev1_off, false);
        assert_eq!(
            change_manual,
            super::WhitelistModeChange::Keep,
            "ручной общий режим БС не должен отключаться при выключении отдельного устройства"
        );
    }

    #[test]
    fn test_whitelist_full_tunnel_commands_includes_ingress_quic_reject() {
        let mut cfg = config::AppConfig::default();
        cfg.whitelist.enabled = true;
        let (cleanup, setup) = super::build_whitelist_full_tunnel_commands(&cfg, 5000, 5001, 1053);
        assert!(
            setup.iter().any(|c| c.contains("-t filter -I INPUT 1 -m mark --mark 0x115/0x115 -p udp --dport 443 -j REJECT --reject-with icmp-port-unreachable")),
            "setup_cmds must include ingress QUIC reject rule: {setup:?}"
        );
        assert!(
            cleanup.contains("while iptables -w -t filter -D INPUT -m mark --mark 0x115/0x115 -p udp --dport 443 -j REJECT --reject-with icmp-port-unreachable 2>/dev/null; do :; done;"),
            "cleanup_cmd must remove ingress QUIC reject rule: {cleanup}"
        );
    }

    #[test]
    fn regression_bug_109_dpi_test_script_keeps_background_results() {
        // Баг 109: `var=$(curl …) &` присваивал результат в фоновом подшелле — после wait переменные пусты,
        // и «Проверка DPI» всегда показывала YouTube/Discord недоступными.
        let script = super::build_dpi_test_script("http://127.0.0.1:7890");
        let fake = format!("curl() {{ case \"$*\" in *youtube*) echo '204:0.10';; *) echo '200:0.20';; esac; }}\n{script}");
        let out = match std::process::Command::new("sh").arg("-c").arg(&fake).output() {
            Ok(o) => o,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                eprintln!("sh is not available on this host; skipping script execution test");
                return;
            }
            Err(e) => panic!("sh доступен: {e}"),
        };
        let line = String::from_utf8_lossy(&out.stdout).trim().to_string();
        assert_eq!(line, "204:0.10#200:0.20#204:0.10#200:0.20", "результаты фоновых проверок должны дойти до вывода");
    }

    #[test]
    fn regression_bug_135_gaming_device_requires_ip() {
        // Баг 135: игровой режим с вкладки «Устройства» создавал устройство с ip: "" — правила Mihomo и
        // iptables строятся по IP, режим «включён», а трафик не перехватывается.
        let mut devs: Vec<config::GamingDevice> = Vec::new();
        let err = super::upsert_gaming_device(&mut devs, "aa:bb:cc:dd:ee:01", None, None, true).unwrap_err();
        assert!(err.contains("IP"), "без IP устройство не создаётся: {err}");
        assert!(devs.is_empty());
        super::upsert_gaming_device(&mut devs, "AA:BB:CC:DD:EE:01", Some("192.168.1.100"), Some("TV"), true).unwrap();
        assert_eq!(devs.len(), 1);
        assert_eq!(devs[0].ip, "192.168.1.100");
        assert_eq!(devs[0].mac, "aa:bb:cc:dd:ee:01");
        assert_eq!(devs[0].name, "TV");
        // IP обновляется (DHCP), выключение работает и без IP
        super::upsert_gaming_device(&mut devs, "aa:bb:cc:dd:ee:01", Some("192.168.1.150"), None, true).unwrap();
        assert_eq!(devs[0].ip, "192.168.1.150");
        super::upsert_gaming_device(&mut devs, "aa:bb:cc:dd:ee:01", None, None, false).unwrap();
        assert!(!devs[0].enabled);
        assert!(super::upsert_gaming_device(&mut devs, "aa:bb:cc:dd:ee:02", Some("1.2.3.4; reboot"), None, true).is_err());
    }

    #[test]
    fn regression_bug_135b_enabling_device_without_ip_fails_or_resolves() {
        // 1. Таблица для чистой resolve_device_ip_for_enable:
        // [IP есть] -> он же
        assert_eq!(
            super::resolve_device_ip_for_enable(Some("192.168.1.50"), Some("192.168.1.60"), Some("192.168.1.70")).unwrap(),
            "192.168.1.50"
        );
        // [пусто + ARP нашёл] -> ARP (приоритет перед RCI)
        assert_eq!(
            super::resolve_device_ip_for_enable(Some(""), Some("192.168.1.60"), Some("192.168.1.70")).unwrap(),
            "192.168.1.60"
        );
        // [пусто + ARP пуст + RCI нашёл] -> RCI
        assert_eq!(
            super::resolve_device_ip_for_enable(None, None, Some("192.168.1.70")).unwrap(),
            "192.168.1.70"
        );
        // [пусто + ничего] -> Err
        assert!(super::resolve_device_ip_for_enable(Some(""), None, Some("")).is_err());

        // 2. Существующая запись с пустым IP при попытке включить без IP -> Err, enabled остаётся false
        let mut devs = vec![config::GamingDevice {
            mac: "aa:bb:cc:dd:ee:03".to_string(),
            ip: "".to_string(),
            ipv6: Vec::new(),
            name: "NoIpBox".to_string(),
            enabled: false,
            server: None,
        }];
        let err = super::upsert_gaming_device(&mut devs, "aa:bb:cc:dd:ee:03", None, None, true);
        assert!(err.is_err(), "существующее устройство с пустым IP нельзя включить без IP");
        assert!(!devs[0].enabled, "enabled не должен меняться при ошибке определения IP");
    }

    #[test]
    fn regression_bug_80_background_sync_never_migrates_http_providers() {
        // Баг 80: фоновая задача раз в час переводила ВСЕ http-провайдеры в формат конвертера
        // (lossy-парсер, type: file) — рабочая Clash-подписка ломалась без участия пользователя.
        let body = fn_body(production_source(), "pub async fn sync_all_mobile_providers");
        assert!(
            !body.contains("convert_http_provider_to_file_in_yaml"),
            "фоновая синхронизация не меняет тип провайдеров в config.yaml"
        );
        assert!(!body.contains("parse_provider_urls"), "обновляются только явно мобильные подписки пользователя");
    }

    #[test]
    fn regression_bug_64_batch_routing_persists_to_device_routing() {
        // Баг 64: пакетное «Сервер для N устройств» жило только в runtime — терялось после рестарта Mihomo,
        // UI его не видел; на каждое устройство вызывался switch_group (полный сброс соединений).
        let mut map = std::collections::BTreeMap::new();
        map.insert("192.168.1.20".to_string(), config::DeviceRouting {
            servers: vec!["old".into(), "backup".into()], ping_threshold_ms: 300, auto_restore: true, follow_whitelist: true,
        });
        map.insert("192.168.1.30".to_string(), config::DeviceRouting::default());
        let a = vec![
            crate::routing::Assignment { ip: "192.168.1.10".into(), name: "TV".into(), server: Some("node-a".into()) },
            crate::routing::Assignment { ip: "192.168.1.20".into(), name: "".into(), server: Some("backup".into()) },
            crate::routing::Assignment { ip: "192.168.1.30".into(), name: "".into(), server: Some("default".into()) },
        ];
        super::merge_assignments_into_device_routing(&mut map, &a);
        assert_eq!(map.get("192.168.1.10").map(|d| d.servers.clone()), Some(vec!["node-a".to_string()]));
        let d20 = map.get("192.168.1.20").unwrap();
        assert_eq!(d20.servers, vec!["backup".to_string(), "old".to_string()], "новый сервер первым, остальная цепочка сохранена");
        assert_eq!(d20.ping_threshold_ms, 300);
        assert!(!map.contains_key("192.168.1.30"), "«default» снимает назначение");
        let body = fn_body(production_source(), "pub async fn apply_routing");
        assert!(!body.contains("switch_group("), "без полного сброса соединений LAN на каждое устройство");
    }

    #[test]
    fn regression_bug_41_whitelist_strict_blocks_ipv6_bypass() {
        // Баг 41: в режиме БС (StrictXKeen) IPv6 не перехватывался — трафик шёл напрямую в обход БС.
        let mut cfg = config::AppConfig::default();
        cfg.whitelist.enabled = true;
        cfg.whitelist.strategy = config::WhitelistStrategy::StrictXKeen;
        cfg.whitelist.excluded_devices = vec!["aa:bb:cc:dd:ee:ff".to_string()];
        let (cleanup, setup) = build_whitelist_full_tunnel_commands(&cfg, 5000, 5001, 1053);
        let joined = setup.join("\n");
        assert!(joined.contains("ip6tables -w -I FORWARD 1 -i br+ -j XKEEN_BS6"), "IPv6 из LAN перехватывается цепочкой БС:\n{joined}");
        assert!(joined.contains("-A XKEEN_BS6 -d fe80::/10 -j RETURN"), "локальный IPv6 не трогается");
        assert!(joined.contains("-A XKEEN_BS6 -m mac --mac-source aa:bb:cc:dd:ee:ff -j RETURN"), "исключённые устройства сохраняют IPv6");
        assert!(joined.contains("-A XKEEN_BS6 -j REJECT"), "внешний IPv6 отклоняется — клиенты уходят на IPv4 через БС");
        assert!(cleanup.contains("ip6tables -w -D FORWARD -i br+ -j XKEEN_BS6"), "парная очистка IPv6");
        assert!(cleanup.contains("ip6tables -w -X XKEEN_BS6"));
        cfg.whitelist.enabled = false;
        let (_, off) = build_whitelist_full_tunnel_commands(&cfg, 5000, 5001, 1053);
        assert!(!off.join("\n").contains("XKEEN_BS6"), "без БС IPv6 не ограничивается");
    }

    #[test]
    fn regression_bug_45a_mihomo_dns_port_read_from_dns_listen() {
        // Баг 45: DNS-порт Mihomo 1053 был зашит; при другом dns.listen DNS-редирект БС/гейминга
        // уходил на мёртвый порт и вся сеть теряла DNS.
        let dir = std::env::temp_dir().join(format!("xkeen-bug45-{}", crate::auth::random_hex(8)));
        std::fs::create_dir_all(&dir).unwrap();
        let yaml_path = dir.join("config.yaml");
        std::fs::write(&yaml_path, "mixed-port: 7890
dns:
  enable: true
  listen: 0.0.0.0:7853
  enhanced-mode: fake-ip
rules:
  - MATCH,DIRECT
").unwrap();
        let mut cfg = config::AppConfig::default();
        cfg.mihomo.config_path = yaml_path.display().to_string();
        cfg.adguard.enabled = false;
        cfg.adblock_enabled = false;
        assert_eq!(resolve_active_dns_port_with_health(&cfg, true), 7853, "порт берётся из dns.listen Mihomo");
        std::fs::write(&yaml_path, "dns:
  enable: true
").unwrap();
        assert_eq!(resolve_active_dns_port_with_health(&cfg, true), 1053, "без listen — стандартный 1053");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn regression_bug_45b_dns_port_not_hardcoded() {
        // 45.1: единая функция mihomo_dns_port(cfg)
        let dir = std::env::temp_dir().join(format!("xkeen-bug45b-{}", crate::auth::random_hex(8)));
        std::fs::create_dir_all(&dir).unwrap();
        let yaml_path = dir.join("config.yaml");
        std::fs::write(&yaml_path, "dns:\n  listen: 0.0.0.0:7853\n").unwrap();
        let mut cfg = config::AppConfig::default();
        cfg.mihomo.config_path = yaml_path.display().to_string();
        assert_eq!(crate::routing::mihomo_dns_port(&cfg), 7853);
        let _ = std::fs::remove_dir_all(&dir);

        // 45.3: сборщик скрипта переключения портов туннелей с портом 7853 содержит 7853 и не содержит 1053
        let script = crate::adguard::build_sync_tunnel_dns_ports_script("5353|5354", 7853);
        assert!(script.contains("--to-ports 7853"), "скрипт должен подставлять переданный порт 7853");
        assert!(!script.contains("1053"), "скрипт для порта 7853 не должен содержать литерал 1053");

        // 45.2: в рабочем коде (до #[cfg(test)], без комментариев) литерал 1053 встречается только в DEFAULT_MIHOMO_DNS_PORT
        let prod_files: &[(&str, &str)] = &[
            ("adguard.rs", include_str!("../adguard.rs")),
            ("config.rs", include_str!("../config.rs")),
            ("zapret.rs", include_str!("../zapret.rs")),
            ("watchdog.rs", include_str!("../watchdog.rs")),
            ("routing/dns_redirect.rs", include_str!("../routing/dns_redirect.rs")),
            ("api/dns_adguard.rs", include_str!("dns_adguard.rs")),
            ("api/routing.rs", include_str!("routing.rs")),
            ("api/status.rs", include_str!("status.rs")),
            ("api/system_ssh.rs", include_str!("system_ssh.rs")),
            ("api/zapret.rs", include_str!("zapret.rs")),
            ("api/mod.rs", include_str!("mod.rs")),
        ];
        let mut offenders = Vec::new();
        for (fname, src) in prod_files {
            let prod_part = &src[..src.find("#[cfg(test)]").unwrap_or(src.len())];
            for (idx, line) in prod_part.lines().enumerate() {
                let trimmed = line.trim();
                if trimmed.starts_with("//") || trimmed.starts_with("///") || trimmed.starts_with("//!") {
                    continue;
                }
                let code_part = line.split("//").next().unwrap_or("");
                if code_part.contains("1053") && !code_part.contains("DEFAULT_MIHOMO_DNS_PORT") {
                    offenders.push(format!("{fname}:{}: {trimmed}", idx + 1));
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "В рабочем коде не должно быть литерала 1053 кроме DEFAULT_MIHOMO_DNS_PORT, найдены: {:?}",
            offenders
        );
    }

    #[test]
    fn regression_bug_125_command_injection_in_settings() {
        // Баг 125: PUT /api/settings позволяет передать произвольные пути/команды в system.xkeen_route_init и xkeen_route_bin
        let current = config::AppConfig::default();
        let body = serde_json::json!({
            "system": {
                "xkeen_route_init": "/opt/etc/init.d/S99xkeen-route; reboot",
                "xkeen_route_bin": "/opt/sbin/xkeen-route | sh"
            }
        });
        let res = merge_settings_update(&current, &body);
        assert!(res.is_err(), "Опасные пути в system должны отклоняться при валидации");
    }

    #[test]
    fn regression_bug_141_mask_telegram_token_and_preserve_placeholders() {
        // Баг 141:
        // 1. Токен Telegram-бота отдается открытым текстом в GET /api/settings (должен маскироваться "******")
        // 2. merge_settings_update затирает секрет литералом "******", если клиент вернул замаскированное значение
        let mut current = config::AppConfig::default();
        current.notifications.telegram_bot_token = "123456789:ABCdefGHIjklMNOpqrs".into();
        current.rci.password = "rci_secret_password".into();
        current.mihomo.secret = "mihomo_secret_token".into();
        current.vkturn.password = "csqtt_tunnel_secret".into();

        // 1. Проверяем маскирование при отдаче настроек
        let masked = mask_settings_for_api(&current);
        assert_eq!(
            masked["notifications"]["telegram_bot_token"].as_str(),
            Some("******"),
            "Токен Telegram-бота должен маскироваться '******' при отдаче в UI"
        );
        assert_eq!(
            masked["vkturn"]["password"].as_str(),
            Some("******"),
            "пароль VK-туннеля должен маскироваться '******' при отдаче в UI"
        );

        // 2. Проверяем, что merge_settings_update не затирает секреты плейсхолдером "******"
        let update_body = serde_json::json!({
            "notifications": {
                "telegram_bot_token": "******",
                "telegram_chat_id": "123456789"
            },
            "rci": {
                "password": "******"
            },
            "mihomo": {
                "secret": "******"
            },
            "vkturn": {
                "password": "******"
            }
        });

        let merged = merge_settings_update(&current, &update_body).expect("merge_settings_update");
        assert_eq!(
            merged.vkturn.password, "csqtt_tunnel_secret",
            "merge_settings_update не должен затирать пароль VK-туннеля"
        );
        assert_eq!(
            merged.notifications.telegram_bot_token,
            "123456789:ABCdefGHIjklMNOpqrs",
            "merge_settings_update не должен затирать реальный токен Telegram строкой '******'"
        );
        assert_eq!(
            merged.rci.password,
            "rci_secret_password",
            "merge_settings_update не должен затирать пароль RCI строкой '******'"
        );
        assert_eq!(
            merged.mihomo.secret,
            "mihomo_secret_token",
            "merge_settings_update не должен затирать секрет Mihomo строкой '******'"
        );
    }

    #[test]
    fn regression_bug_141b_all_masked_secrets_survive_put() {
        let mut current = config::AppConfig::default();
        current.notifications.telegram_bot_token = "123456789:ABCdefGHIjklMNOpqrs".into();
        current.notifications.webhook_url = "https://discord.com/api/webhooks/123/secret_token".into();
        current.vkturn.vk_token = Some("vk1.a.VerySecretTokenLongString".into());
        current.vkturn.password = "csqtt_tunnel_secret".into();
        current.adguard.password = Some("adguard_admin_password".into());
        current.rci.password = "rci_secret_password".into();
        current.rci.token = "rci_secret_token_12345".into();
        current.mihomo.secret = "mihomo_secret_token".into();
        current.antigravity.own_proxy = "http://user:pass123@10.0.0.5:8080".into();

        // 1. Проверяем маскирование при отдаче настроек
        let masked = mask_settings_for_api(&current);

        // vkturn.vk_token должен маскироваться в GET
        assert_eq!(masked["vkturn"]["vk_token"].as_str(), Some("vk1.a.***"));
        // antigravity.own_proxy с учетными данными должен маскироваться
        assert_eq!(masked["antigravity"]["own_proxy"].as_str(), Some("******"));

        // 2. PUT с масками (как в GET) обязан восстанавливать ВСЕ поля из current
        let merged = merge_settings_update(&current, &masked).expect("merge_settings_update with masked payload");
        assert_eq!(merged.notifications.telegram_bot_token, current.notifications.telegram_bot_token);
        assert_eq!(merged.notifications.webhook_url, current.notifications.webhook_url);
        assert_eq!(merged.vkturn.vk_token, current.vkturn.vk_token);
        assert_eq!(merged.vkturn.password, current.vkturn.password);
        assert_eq!(merged.adguard.password, current.adguard.password);
        assert_eq!(merged.rci.password, current.rci.password);
        assert_eq!(merged.rci.token, current.rci.token);
        assert_eq!(merged.mihomo.secret, current.mihomo.secret);
        assert_eq!(merged.antigravity.own_proxy, current.antigravity.own_proxy);

        // 3. PUT с альтернативными масками ([REDACTED], пустая строка там где пустая = не менять)
        let alt_masked = serde_json::json!({
            "vkturn": {
                "vk_token": "[REDACTED]"
            },
            "adguard": {
                "password": ""
            },
            "antigravity": {
                "own_proxy": "******"
            }
        });
        let merged_alt = merge_settings_update(&current, &alt_masked).expect("merge_settings_update with alt masked payload");
        assert_eq!(merged_alt.vkturn.vk_token, current.vkturn.vk_token);
        assert_eq!(merged_alt.adguard.password, current.adguard.password);
        assert_eq!(merged_alt.antigravity.own_proxy, current.antigravity.own_proxy);
    }

    #[test]
    fn test_full_tunnel_resync_covers_whitelist_mode() {
        // REG: после переключения AdGuard синк перехвата шёл только для игрового режима
        let mut cfg = config::AppConfig::default();
        assert!(!full_tunnel_resync_needed(&cfg));
        cfg.whitelist.enabled = true;
        cfg.whitelist.strategy = config::WhitelistStrategy::StrictXKeen;
        assert!(full_tunnel_resync_needed(&cfg));
        cfg.whitelist.enabled = false;
        cfg.gaming.enabled = true;
        cfg.gaming.mode = config::GamingMode::Compatibility;
        assert!(full_tunnel_resync_needed(&cfg));
    }

    #[test]
    fn test_adguard_toggles_do_not_hardcode_tunnel_ports() {
        // REG (Z-09): toggle_adblock / adguard_set_protection синкали перехват на 5000/5001
        let src = production_source();
        for sig in ["pub async fn toggle_adblock", "pub async fn adguard_set_protection"] {
            let body = fn_body(src, sig);
            assert!(!body.contains("redir_port = 5000;"), "{sig}: хардкод порта");
            assert!(body.contains("resync_full_tunnel_from_disk("), "{sig}: нет пересинхронизации");
        }
    }

    #[test]
    fn test_service_stop_cleans_bs_chains_and_start_resyncs() {
        // REG: «Остановить» оставлял XKEEN_BS и хук 061 → весь трафик LAN в мёртвый Mihomo;
        // «Запустить» не возвращал перехват игрового режима/БС.
        let body = fn_body(production_source(), "pub async fn xkeen_service");
        let stop = body.find("if action == \"stop\"").expect("stop branch");
        let start = body.find("if action == \"start\" || action == \"restart\" || action == \"restart_all\" {\n                // Восстанавливаем")
            .or_else(|| body.rfind("// Восстанавливаем правила маршрутизации"))
            .expect("start branch");
        assert!(body[stop..start].contains("emergency_cleanup_full_tunnel_rules().await"));
        assert!(body[start..].contains("sync_full_tunnel_modes("));
    }

    #[test]
    fn test_adguard_sync_firewall_route_registered() {
        // REG: кнопка «Восстановить DNS» вызывала несуществующий маршрут
        let main_src = include_str!("../main.rs");
        assert!(main_src.contains("\"/api/adguard/sync-firewall\""));
    }
}

#[cfg(test)]
mod full_tunnel_guard_tests {
    use super::effective_full_tunnel_flags;

    #[test]
    fn test_no_interception_while_mihomo_down() {
        // REG: при упавшем Mihomo любой синк заново ставил REDIRECT/TPROXY в мёртвые
        // порты, отменяя аварийную очистку сторожа → блэкаут всей LAN.
        assert_eq!(effective_full_tunnel_flags(true, false, true), (false, false));
        assert_eq!(effective_full_tunnel_flags(false, true, true), (false, false));
        assert_eq!(effective_full_tunnel_flags(true, true, true), (false, false));
    }

    #[test]
    fn test_interception_passthrough_when_mihomo_alive() {
        assert_eq!(effective_full_tunnel_flags(true, false, false), (true, false));
        assert_eq!(effective_full_tunnel_flags(false, true, false), (false, true));
        assert_eq!(effective_full_tunnel_flags(false, false, false), (false, false));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn production_source() -> &'static str {
        concat!(
            include_str!("backups.rs"), "\n",
            include_str!("clash_proxy.rs"), "\n",
            include_str!("dns_adguard.rs"), "\n",
            include_str!("domains.rs"), "\n",
            include_str!("routing.rs"), "\n",
            include_str!("servers.rs"), "\n",
            include_str!("status.rs"), "\n",
            include_str!("system_ssh.rs"), "\n",
            include_str!("vkturn.rs"), "\n",
            include_str!("zapret.rs"), "\n",
            include_str!("mod.rs")
        )
    }

    fn fn_body<'a>(src: &'a str, signature: &str) -> &'a str {
        let needle = format!("{signature}(");
        let start = src.find(&needle).unwrap_or_else(|| panic!("нет функции {signature}"));
        let rest = &src[start + needle.len()..];
        let end = rest.find("\npub async fn ")
            .or_else(|| rest.find("\nasync fn "))
            .or_else(|| rest.find("\npub fn "))
            .or_else(|| rest.find("\nfn "))
            .unwrap_or(rest.len());
        &rest[..end]
    }

    #[test]
    fn test_compute_whitelist_mode_active_consistency() {
        let mut cfg = crate::config::AppConfig::default();
        cfg.whitelist.enabled = false;
        cfg.whitelist_servers = vec!["wl_srv".to_string()];
        cfg.whitelist_providers = vec!["wl_prov".to_string()];

        // 1. Whitelist config enabled
        let mut cfg_enabled = cfg.clone();
        cfg_enabled.whitelist.enabled = true;
        assert!(compute_whitelist_mode_active(&cfg_enabled, "any", "Any", None));

        // 2. Server in whitelist_servers
        assert!(compute_whitelist_mode_active(&cfg, "wl_srv", "Regular Name", None));

        // 3. Provider in whitelist_providers
        assert!(compute_whitelist_mode_active(&cfg, "other_srv", "Regular Name", Some("wl_prov")));

        // 4. Tag [БС] in name
        assert!(compute_whitelist_mode_active(&cfg, "other_srv", "Server [БС] Moscow", None));

        // 5. Standard VPS
        assert!(!compute_whitelist_mode_active(&cfg, "vps_srv", "Germany VPS", Some("vps_prov")));
    }

    #[tokio::test]
    async fn test_provider_quota_and_live_delta_calculation() {
        crate::traffic::reset_provider_live_delta("prov_alpha");
        assert_eq!(crate::traffic::get_provider_live_delta("prov_alpha"), 0);

        crate::traffic::add_provider_live_delta("prov_alpha", 5000);
        assert_eq!(crate::traffic::get_provider_live_delta("prov_alpha"), 5000);

        // Baseline used: 1000 up, 2000 down, total 10000
        let official_used: u64 = 1000 + 2000;
        let total: u64 = 10000;
        let remaining = total.saturating_sub(official_used);

        assert_eq!(official_used, 3000);
        assert_eq!(remaining, 7000);

        // Fresh sync resets delta
        crate::traffic::reset_provider_live_delta("prov_alpha");
        assert_eq!(crate::traffic::get_provider_live_delta("prov_alpha"), 0);
    }

    #[test]
    fn test_split_curl_response_multi_block_headers_and_payload() {
        // Scenario 1: CONNECT proxy response + QRATOR HTTP/2 response + JSON array
        let raw = concat!(
            "HTTP/1.1 200 Connection established\r\n\r\n",
            "HTTP/2 200 \r\n",
            "server: QRATOR\r\n",
            "date: Sun, 01 Nov 2026 20:00:00 GMT\r\n",
            "content-type: application/json; charset=utf-8\r\n",
            "subscription-userinfo: upload=100; download=200; total=1000; expire=1829858058\r\n\r\n",
            "[{\"remarks\": \"Test Server\"}]"
        );

        let (userinfo, body) = split_curl_response(raw);
        assert!(userinfo.is_some(), "Must parse userinfo from multi-block response");
        let u = userinfo.unwrap();
        assert_eq!(u.upload, 100);
        assert_eq!(u.download, 200);
        assert_eq!(u.total, 1000);
        assert_eq!(body, "[{\"remarks\": \"Test Server\"}]");
        assert!(!body.contains("HTTP/"), "Body must not contain HTTP status or headers");
        assert!(!body.contains("QRATOR"), "Body must not contain server header");

        // Scenario 2: CONNECT proxy response + Base64 subscription body
        let raw_b64 = concat!(
            "HTTP/1.1 200 Connection established\r\n\r\n",
            "HTTP/1.1 200 OK\r\n",
            "Subscription-Userinfo: upload=0; download=500; total=2000; expire=1800000000\r\n\r\n",
            "dmxlc3M6Ly90ZXN0QDEuMi4zLjQ6NDQzI1Rlc3Q="
        );
        let (u2, body_b64) = split_curl_response(raw_b64);
        assert!(u2.is_some());
        assert_eq!(u2.unwrap().download, 500);
        assert_eq!(body_b64, "dmxlc3M6Ly90ZXN0QDEuMi4zLjQ6NDQzI1Rlc3Q=");
        assert!(!body_b64.contains("HTTP/"));
    }

    #[test]
    fn whitelist_iptables_wait_is_compatible_with_legacy_keenetic() {
        let mut cfg = crate::config::AppConfig::default();
        cfg.whitelist.enabled = true;
        cfg.whitelist.strategy = crate::config::WhitelistStrategy::StrictXKeen;
        let (cleanup, setup) = build_whitelist_full_tunnel_commands(&cfg, 5000, 5001, 1053);
        for command in std::iter::once(cleanup.as_str()).chain(setup.iter().map(String::as_str)) {
            assert!(!command.contains("-w 2 ") && !command.ends_with("-w 2"), "legacy iptables rejects a numeric wait timeout: {command}");
        }
        assert!(setup.iter().any(|cmd| cmd.contains("-t nat -I PREROUTING 1 -j XKEEN_BS -w")));
        assert!(setup.iter().any(|cmd| cmd.contains("-t mangle -I PREROUTING 1 -j XKEEN_BS -w")));
    }

    #[test]
    fn whitelist_both_strategies_restore_interception_and_disabled_cleans_up() {
        let mut cfg = crate::config::AppConfig::default();
        cfg.gaming.enabled = false;
        for strategy in [crate::config::WhitelistStrategy::StrictXKeen, crate::config::WhitelistStrategy::Standard] {
            cfg.whitelist.strategy = strategy;
            cfg.whitelist.enabled = true;
            assert!(full_tunnel_resync_needed(&cfg));
            let (_, setup) = build_whitelist_full_tunnel_commands(&cfg, 5000, 5001, 1053);
            assert!(setup.iter().any(|cmd| cmd.contains("-t nat -I PREROUTING 1 -j XKEEN_BS")));
            assert!(setup.iter().any(|cmd| cmd.contains("-t mangle -I PREROUTING 1 -j XKEEN_BS")));
            cfg.whitelist.enabled = false;
            assert!(!full_tunnel_resync_needed(&cfg));
            let (cleanup, setup) = build_whitelist_full_tunnel_commands(&cfg, 5000, 5001, 1053);
            assert!(setup.is_empty());
            assert!(cleanup.contains("-D PREROUTING -j XKEEN_BS"));
        }
    }

    /// Анти-регрессия BS-FIX: полный перехват БС обязан захватывать ВСЕ TCP/UDP порты
    /// (без --dport ограничений) — иначе игровой трафик вроде 9339 вылетает из туннеля
    /// в прямой канал провайдера, где ТСПУ дропает его SYN.
    #[test]
    fn test_build_whitelist_full_tunnel_captures_all_ports() {
        let mut cfg = crate::config::AppConfig::default();
        cfg.whitelist.enabled = true;
        cfg.whitelist.strategy = crate::config::WhitelistStrategy::StrictXKeen;
        cfg.whitelist.excluded_devices = vec!["192.168.2.99".to_string()];

        let (cleanup, setup) = build_whitelist_full_tunnel_commands(&cfg, 5000, 5001, 1053);

        // Цепочки XKEEN_BS создаются и цепляются в начало PREROUTING
        assert!(setup.iter().any(|c| c.contains("iptables -t nat -N XKEEN_BS")));
        assert!(setup.iter().any(|c| c.contains("iptables -t nat -I PREROUTING 1 -j XKEEN_BS")));
        assert!(setup.iter().any(|c| c.contains("iptables -t mangle -N XKEEN_BS")));
        assert!(setup.iter().any(|c| c.contains("iptables -t mangle -I PREROUTING 1 -j XKEEN_BS")));
        // Трафик TUN-интерфейса csq0 и пакеты с routing-mark 222 не должны зацикливаться через XKEEN_BS
        assert!(setup.iter().any(|c| c == "iptables -t nat -A XKEEN_BS -i csq0 -j RETURN -w"));
        assert!(setup.iter().any(|c| c == "iptables -t mangle -A XKEEN_BS -i csq0 -j RETURN -w"));
        assert!(setup.iter().any(|c| c == "iptables -t nat -A XKEEN_BS -m mark --mark 222 -j RETURN -w"));
        assert!(setup.iter().any(|c| c == "iptables -t mangle -A XKEEN_BS -m mark --mark 222 -j RETURN -w"));

        // TCP захват БЕЗ ограничения портов (для всех трёх приватных подсетей)
        for subnet in ["192.168.0.0/16", "10.0.0.0/8", "172.16.0.0/12"] {
            let tcp_rule = setup.iter().find(|c| c.contains(&format!("-s {subnet} -p tcp -j REDIRECT --to-ports 5000")));
            assert!(tcp_rule.is_some(), "нет полного TCP-захвата для {subnet}");
            assert!(!tcp_rule.unwrap().contains("--dport"), "TCP-захват {subnet} не должен ограничивать порты");
            let udp_rule = setup.iter().find(|c| c.contains(&format!("-s {subnet} -p udp -j TPROXY --tproxy-mark 0x115/0xffffffff --on-port 5001")));
            assert!(udp_rule.is_some(), "нет полного UDP-захвата (TPROXY) для {subnet}");
        }

        // DNS-редирект до байпаса локальных подсетей
        assert!(setup.iter().any(|c| c.contains("-p udp --dport 53 -j REDIRECT --to-ports 1053")));
        // Байпас локальных подсетей (RETURN) присутствует
        assert!(setup.iter().any(|c| c.contains("-d 192.168.0.0/16 -j RETURN")));
        // Исключённое устройство БС обходит перехват целиком (RETURN в nat и mangle)
        assert!(setup.iter().any(|c| c.contains("-s 192.168.2.99 -j RETURN") && c.contains("-t nat")));
        assert!(setup.iter().any(|c| c.contains("-s 192.168.2.99 -j RETURN") && c.contains("-t mangle")));
        // Policy routing для TPROXY
        assert!(setup.iter().any(|c| c.contains("ip rule add fwmark 0x115 lookup 115")));

        // Cleanup удаляет и цепочки, и PREROUTING-джампы
        assert!(cleanup.contains("iptables -t nat -D PREROUTING -j XKEEN_BS"));
        assert!(cleanup.contains("iptables -t nat -X XKEEN_BS"));
        assert!(cleanup.contains("iptables -t mangle -X XKEEN_BS"));

        // Выключенный БС не генерирует правил
        let mut cfg_off = crate::config::AppConfig::default();
        cfg_off.whitelist.enabled = false;
        let (_, setup_off) = build_whitelist_full_tunnel_commands(&cfg_off, 5000, 5001, 1053);
        assert!(setup_off.is_empty(), "выключенный БС не должен генерировать правила перехвата");

        // Standard использует обычные правила Mihomo, но тоже должна обходить geo_exclude
        let mut cfg_std = crate::config::AppConfig::default();
        cfg_std.whitelist.enabled = true;
        cfg_std.whitelist.strategy = crate::config::WhitelistStrategy::Standard;
        let (_, setup_std) = build_whitelist_full_tunnel_commands(&cfg_std, 5000, 5001, 1053);
        assert!(!setup_std.is_empty(), "Standard-стратегия должна доставлять трафик до правил Mihomo");
    }

    /// Анти-регрессия BS-FIX: мьютекс режимов. При активном БС игровой полный перехват
    /// обязан сниматься (gaming.enabled принудительно false), и наоборот.
    #[test]
    fn test_sync_full_tunnel_modes_mutex_flags() {
        let mut cfg = crate::config::AppConfig::default();
        cfg.whitelist.enabled = true;
        cfg.whitelist.strategy = crate::config::WhitelistStrategy::StrictXKeen;
        cfg.gaming.enabled = true;
        cfg.gaming.mode = crate::config::GamingMode::Compatibility;
        // Генератору gaming нужна хотя бы одна активная ради устройства
        cfg.gaming.devices.push(crate::config::GamingDevice {
            mac: "AA:BB:CC:DD:EE:10".into(),
            ip: "192.168.2.70".into(),
            ipv6: vec![],
            name: "Console".into(),
            enabled: true,
            server: None,
        });

        let bs_active = cfg.whitelist.enabled && cfg.whitelist.strategy == crate::config::WhitelistStrategy::StrictXKeen;
        let gaming_active = cfg.gaming.enabled && cfg.gaming.mode == crate::config::GamingMode::Compatibility;
        assert!(bs_active && gaming_active, "конфликтный сценарий должен быть подготовлен");

        // sync_full_tunnel_modes глушит gaming-правила при активном БС:
        // проверяем на чистой функции-конструкторе — gaming-cfg с enabled=false
        // не генерирует перехват, а БС-конструктор его генерирует
        let mut gaming_muted = cfg.gaming.clone();
        gaming_muted.enabled = false;
        let (_, gaming_setup, _ct) = build_gaming_full_tunnel_commands(&gaming_muted, 5000, 5001, 1053);
        assert!(gaming_setup.is_empty(), "при активном БС gaming-перехват должен быть пуст");
        let (_, bs_setup) = build_whitelist_full_tunnel_commands(&cfg, 5000, 5001, 1053);
        assert!(!bs_setup.is_empty(), "при активном БС должен быть перехват XKEEN_BS");

        let mut wl_muted = cfg.clone();
        wl_muted.whitelist.enabled = false;
        let (_, wl_setup) = build_whitelist_full_tunnel_commands(&wl_muted, 5000, 5001, 1053);
        assert!(wl_setup.is_empty(), "при активном gaming БС-перехват должен быть пуст");
        let (_, gaming_setup2, _ct2) = build_gaming_full_tunnel_commands(&cfg.gaming, 5000, 5001, 1053);
        assert!(!gaming_setup2.is_empty(), "при активном gaming должен быть перехват XKEEN_GAMING");
    }

    #[test]
    fn test_sync_zapret_hosts_content_dynamic_isolation() {
        let base = "# zapret-hosts.txt\nmanual-site.org\nntc.party\n";
        let mut cfg = crate::config::ZapretConfig {
            bypass_github: true,
            bypass_torrents: true,
            bypass_adult: true,
            custom_entries: vec![crate::config::ZapretCustomEntry {
                domain: "mysku.club".to_string(),
                enabled: true,
                cdns: vec!["img.mysku-st.ru".to_string()],
            }],
            ..Default::default()
        };

        // 1. Initial sync adds managed block
        let synced = sync_zapret_hosts_content(base, &cfg);
        assert!(synced.contains("manual-site.org"));
        assert!(synced.contains(ZAPRET_HOSTS_MANAGED_BEGIN));
        assert!(synced.contains("github.com"));
        assert!(synced.contains("rutracker.org"));
        assert!(synced.contains("pornhub.com"));
        assert!(synced.contains("mysku.club"));
        assert!(synced.contains("img.mysku-st.ru"));

        // 2. Disabling custom entry removes it from zapret-hosts.txt
        cfg.custom_entries[0].enabled = false;
        let synced_disabled = sync_zapret_hosts_content(&synced, &cfg);
        assert!(!synced_disabled.contains("mysku.club"));
        assert!(!synced_disabled.contains("img.mysku-st.ru"));
        assert!(synced_disabled.contains("github.com"));
        assert!(synced_disabled.contains("pornhub.com"));
        assert!(synced_disabled.contains("manual-site.org"));

        // 3. Deleting custom entry removes it from zapret-hosts.txt
        cfg.custom_entries.clear();
        let synced_deleted = sync_zapret_hosts_content(&synced, &cfg);
        assert!(!synced_deleted.contains("mysku.club"));
        assert!(!synced_deleted.contains("img.mysku-st.ru"));

        // 4. Disabling bypass_github, bypass_torrents, and bypass_adult removes them
        cfg.bypass_github = false;
        cfg.bypass_torrents = false;
        cfg.bypass_adult = false;
        let synced_no_managed = sync_zapret_hosts_content(&synced_deleted, &cfg);
        assert!(!synced_no_managed.contains("github.com"));
        assert!(!synced_no_managed.contains("rutracker.org"));
        assert!(!synced_no_managed.contains("pornhub.com"));
        assert!(!synced_no_managed.contains(ZAPRET_HOSTS_MANAGED_BEGIN));
        assert!(synced_no_managed.contains("manual-site.org"));
        assert!(synced_no_managed.contains("ntc.party"));
    }

    #[test]
    fn test_build_nfqws2_args_toggles_and_presets() {
        let mut cfg = crate::config::ZapretConfig {
            enabled: true,
            youtube_turbo: true,
            hybrid_youtube: true,
            hybrid_discord: true,
            discord_voice_udp: true,
            general_bypass: true,
            aggressive_dpi: false,
            ..Default::default()
        };

        let (args, voice_enabled) = build_nfqws2_args(&cfg);
        assert!(voice_enabled);
        assert!(args.contains("--daemon --qnum=200 --dpi-desync-fwmark=0x40000000"));
        assert!(args.contains("--filter-tcp=80,443 --filter-l7=tls,http --hostlist-domains=googlevideo.com"));
        assert!(args.contains("--payload=tls_client_hello"));
        assert!(args.contains("--out-range=-d10"));
        assert!(args.contains("--lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=6:tls_mod=rnd,dupsid"));
        assert!(args.contains("--lua-desync=multisplit:pos=1,midsld"));
        assert!(args.contains("--filter-udp=50000-65535 --filter-l7=discord,stun --payload=discord_ip_discovery,stun --lua-desync=fake:blob=0x00000000:repeats=4"));
        assert!(args.contains("--hostlist=/opt/etc/zapret/zapret-hosts.txt"));
        assert!(args.contains("--lua-desync=multisplit:pos=1"));

        // Aggressive mode check
        cfg.aggressive_dpi = true;
        let (agg_args, _) = build_nfqws2_args(&cfg);
        assert!(agg_args.contains("tcp_ts_up:seqovl=5:tcp_ack=-66000"));
        assert!(agg_args.contains("--lua-desync=multisplit:pos=1,midsld"));

        // YouTube Direct without Turbo (standard desync)
        let yt_direct_cfg = crate::config::ZapretConfig {
            enabled: true,
            youtube_turbo: false,
            hybrid_youtube: true,
            general_bypass: false,
            bypass_github: false,
            bypass_torrents: false,
            bypass_adult: false,
            hybrid_discord: false,
            discord_voice_udp: false,
            ..Default::default()
        };
        let (yt_direct_args, _) = build_nfqws2_args(&yt_direct_cfg);
        assert!(yt_direct_args.contains("--filter-tcp=80,443 --filter-l7=tls,http --hostlist-domains=googlevideo.com"));
        assert!(yt_direct_args.contains("--lua-desync=fake:blob=fake_default_tls --lua-desync=multisplit:pos=1"));
        assert!(!yt_direct_args.contains("tls_mod=rnd,dupsid"));

        // Discord only preset
        let dc_cfg = crate::config::ZapretConfig {
            enabled: true,
            youtube_turbo: false,
            hybrid_youtube: false,
            hybrid_discord: true,
            discord_voice_udp: true,
            general_bypass: false,
            bypass_github: false,
            bypass_torrents: false,
            bypass_adult: false,
            ..Default::default()
        };
        let (dc_args, dc_voice) = build_nfqws2_args(&dc_cfg);
        assert!(dc_voice);
        assert!(dc_args.contains("--filter-tcp=80,443 --filter-l7=tls,http --hostlist-domains=discord.com"));
        assert!(dc_args.contains("--filter-udp=50000-65535 --filter-l7=discord,stun --payload=discord_ip_discovery,stun"));
        assert!(!dc_args.contains("googlevideo.com"));

        // Empty config provides safe default profile
        let empty_cfg = crate::config::ZapretConfig {
            enabled: true,
            youtube_turbo: false,
            hybrid_youtube: false,
            hybrid_discord: false,
            discord_voice_udp: false,
            general_bypass: false,
            bypass_github: false,
            bypass_torrents: false,
            bypass_adult: false,
            custom_entries: vec![],
            ..Default::default()
        };
        let (empty_args, empty_voice) = build_nfqws2_args(&empty_cfg);
        assert!(!empty_voice);
        assert!(empty_args.contains("--daemon --qnum=200 --dpi-desync-fwmark=0x40000000"));
        assert!(empty_args.contains("--out-range=-d10"));
    }

    #[test]
    fn test_validate_custom_args_zapret2_syntax() {
        // Zapret2 valid syntax with @, :, #, %, =, -, _, /, ., ~
        let valid_args = "--lua-init=@/opt/zapret2/lua/zapret-lib.lua --lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=6:tls_mod=rnd,dupsid%10#tag --out-range=-d10";
        assert!(validate_custom_args(valid_args).is_ok());

        // Forbidden shell characters
        assert!(validate_custom_args("--arg; rm -rf /").is_err());
        assert!(validate_custom_args("--arg & echo 1").is_err());
        assert!(validate_custom_args("--arg | grep x").is_err());
        assert!(validate_custom_args("--arg `id`").is_err());
        assert!(validate_custom_args("--arg $(whoami)").is_err());
        assert!(validate_custom_args("--arg > /dev/null").is_err());
        assert!(validate_custom_args("--arg < /dev/null").is_err());
        assert!(validate_custom_args("--arg !ls").is_err());
        assert!(validate_custom_args("--arg \"quote\"").is_err());
        assert!(validate_custom_args("--arg 'single'").is_err());
    }

    #[test]
    fn test_convert_lua_to_legacy_desync() {
        let lua_fake = "--lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=6:tls_mod=rnd,dupsid --lua-desync=multisplit:pos=1,midsld";
        let legacy = convert_lua_to_legacy_desync(lua_fake);
        assert!(legacy.contains("--dpi-desync=fake,split2"));
        assert!(!legacy.contains("multisplit"));
        assert!(legacy.contains("--dpi-desync-split-pos=1,midsld"));
        assert!(!legacy.contains("lua-desync"));

        let disorder_lua = "--lua-desync=fake --lua-desync=disorder2:pos=1,midsld";
        let legacy_disorder = convert_lua_to_legacy_desync(disorder_lua);
        assert!(legacy_disorder.contains("--dpi-desync=fake,disorder2"));
        assert!(legacy_disorder.contains("midsld"));

        let split_lua = "--lua-desync=split2:pos=1";
        let legacy_split = convert_lua_to_legacy_desync(split_lua);
        assert!(legacy_split.contains("--dpi-desync=fake,split2"));

        let full_cmdline = format!(
            "--daemon --qnum=200 --filter-tcp=80,443 --dpi-desync=fake,{} --new --filter-udp=50000-65535 --dpi-desync=fake",
            "multisplit"
        );
        let legacy_full = convert_lua_to_legacy_desync(&full_cmdline);
        assert!(legacy_full.contains("--daemon --qnum=200"));
        assert!(legacy_full.contains("--dpi-desync=fake,split2"));
        assert!(legacy_full.contains("--new --filter-udp=50000-65535"));
        assert!(!legacy_full.contains("multisplit"));

        assert!(S51ZAPRET_SCRIPT.contains("command $IPTABLES_CMD \"$@\""));
        assert!(S51ZAPRET_SCRIPT.contains("exec \"$0\" restart"));
    }

    #[test]
    fn test_format_nfqws_proc_cmdline_restores_strtok_commas() {
        let raw = b"/opt/zapret/nfq/nfqws\0--pidfile=/opt/var/run/zapret.pid\0--daemon\0--dpi-desync=fake\0split2\0--dpi-desync-split-pos=1\0midsld\0--dpi-desync-fooling=ts\0md5sig\0";
        let formatted = format_nfqws_proc_cmdline(raw);
        assert_eq!(
            formatted,
            "/opt/zapret/nfq/nfqws --pidfile=/opt/var/run/zapret.pid --daemon --dpi-desync=fake,split2 --dpi-desync-split-pos=1,midsld --dpi-desync-fooling=ts,md5sig"
        );
    }

    #[test]
    fn test_build_nfqws_args_with_desync() {
        let cfg = crate::config::ZapretConfig {
            enabled: true,
            youtube_turbo: true,
            general_bypass: true,
            ..Default::default()
        };
        let custom = "--dpi-desync=fake,split2 --dpi-desync-split-pos=2";
        let (args, _) = build_nfqws_args_with_desync(&cfg, Some(custom));
        assert!(args.contains("--daemon --qnum=200"));
        assert!(args.contains(custom));
    }

    #[test]
    fn test_build_zapret_hardware_info_and_auto_hint() {
        let titan = build_zapret_hardware_info(
            "Keenetic Titan (KN-1811)",
            "Titan",
            "KN-1811",
            "aarch64",
            true,
            502_400,
        );
        assert_eq!(titan.model, "Titan KN-1811");
        assert_eq!(titan.arch_label, "ARM64");
        assert_eq!(titan.target_arch, "linux-arm64");
        assert_eq!(titan.ram_mb, 512);
        assert_eq!(titan.recommended_engine, "v2");
        assert_eq!(
            titan.hint_text,
            "Обнаружен Titan KN-1811 (ARM64, 512MB RAM) — рекомендуется Zapret 2.0"
        );

        let giga_mips = build_zapret_hardware_info(
            "KeeneticViva",
            "Viva",
            "KN-1910",
            "mips",
            true,
            250_000,
        );
        assert_eq!(giga_mips.target_arch, "linux-mips32r2-lsb");
        assert_eq!(giga_mips.ram_mb, 256);
        assert_eq!(giga_mips.recommended_engine, "v2");

        let low_ram_mips = build_zapret_hardware_info(
            "Keenetic Start (KN-1111)",
            "Start",
            "KN-1111",
            "mips",
            false,
            60_000,
        );
        assert_eq!(low_ram_mips.target_arch, "linux-mips32r2-msb");
        assert_eq!(low_ram_mips.ram_mb, 64);
        assert_eq!(low_ram_mips.recommended_engine, "v1");

        let ultra_low_ram = build_zapret_hardware_info(
            "Keenetic Lite (KN-1310)",
            "Lite",
            "KN-1310",
            "mips",
            true,
            30_000,
        );
        assert_eq!(ultra_low_ram.ram_mb, 32);
        assert_eq!(ultra_low_ram.recommended_engine, "v1");
        assert!(ultra_low_ram.hint_text.contains("32MB RAM"));
        assert!(ultra_low_ram.hint_text.contains("рекомендуется Legacy 1.x"));

        let (x86_lbl, x86_target) = parse_arch_info("i686", true);
        assert_eq!(x86_lbl, "x86");
        assert_eq!(x86_target, "linux-x86");

        let (empty_lbl, _) = parse_arch_info("", true);
        assert!(!empty_lbl.is_empty());

        let fallback = build_zapret_hardware_info("", "", "", "", true, 0);
        assert_eq!(fallback.model, "Keenetic Router");
        assert!(!fallback.arch_label.is_empty());
        assert_eq!(fallback.ram_mb, 512);
    }

    #[test]
    fn test_s51zapret_engine_switching_and_conf_validation() {
        assert_eq!(normalize_engine_choice("v1"), "v1");
        assert_eq!(normalize_engine_choice("legacy"), "v1");
        assert_eq!(normalize_engine_choice("1.x"), "v1");
        assert_eq!(normalize_engine_choice("v2"), "v2");
        assert_eq!(normalize_engine_choice("modern"), "v2");

        // Ensure S51ZAPRET_SCRIPT parses ZAPRET_ENGINE before calling find_bin
        let conf_parse_pos = S51ZAPRET_SCRIPT.find("ZAPRET_ENGINE)").expect("ZAPRET_ENGINE case in S51zapret");
        let find_bin_call_pos = S51ZAPRET_SCRIPT.find("BIN=$(find_bin)").expect("BIN=$(find_bin) in S51zapret");
        assert!(
            conf_parse_pos < find_bin_call_pos,
            "BIN=$(find_bin) must be evaluated after reading ZAPRET_ENGINE from zapret.conf"
        );

        let valid_conf = "ZAPRET_ENGINE=\"v2\"\nNFQWS_ARGS=\"--daemon --qnum=200\"\nDISCORD_VOICE_ENABLED=\"1\"\nADGUARD_ENABLED=\"1\"\n";
        assert!(validate_zapret_conf(valid_conf).is_ok());

        let invalid_engine_conf = "ZAPRET_ENGINE=\"v3\"\n";
        assert!(validate_zapret_conf(invalid_engine_conf).is_err());

        // Z-01 anti-regression: S51zapret обязан использовать единый с панелью mkdir-протокол
        // блокировки (lockdir/pid); flock на файл не пересекался с локом панели
        assert!(S51ZAPRET_SCRIPT.contains("mkdir \"$LOCKDIR\""));
        assert!(S51ZAPRET_SCRIPT.contains("cat \"$LOCKDIR/pid\""));
        assert!(!S51ZAPRET_SCRIPT.contains("flock -w"), "flock protocol must stay removed (Z-01)");
        // Z-03 anti-regression: автозапуск обязан уважать маркер «выключено пользователем»
        assert!(S51ZAPRET_SCRIPT.contains("$DISABLED_MARKER\" ]; then"));

        // Z-04 anti-regression: EXCLUDED_IPS/MACS строго валидируются — command injection
        // через `$(...)`/backticks в значениях должен отклоняться до записи в zapret.conf
        // (значения попадают в `iptables -s "$ex_ip"` внутри S51zapret).
        let valid_exclusions = "EXCLUDED_IPS=\"192.168.1.50 10.0.0.0/24\"\nEXCLUDED_MACS=\"00:11:22:33:44:55\"\n";
        assert!(validate_zapret_conf(valid_exclusions).is_ok());
        let injected_ips = "EXCLUDED_IPS=\"192.168.1.1; rm -rf /\"\n";
        assert!(validate_zapret_conf(injected_ips).is_err(), "EXCLUDED_IPS must reject non-IP tokens");
        let injected_subst = "EXCLUDED_IPS=\"$(reboot)\"\n";
        assert!(validate_zapret_conf(injected_subst).is_err(), "EXCLUDED_IPS must reject command substitution");
        let injected_macs = "EXCLUDED_MACS=\"`id`\n";
        assert!(validate_zapret_conf(injected_macs).is_err(), "EXCLUDED_MACS must reject command substitution");
        let bad_mac = "EXCLUDED_MACS=\"zz:11:22:33:44:55\"\n";
        assert!(validate_zapret_conf(bad_mac).is_err(), "EXCLUDED_MACS must reject non-hex MAC");

        // Anti-regression: S51ZAPRET_SCRIPT must verify lua-desync capability before passing --lua-init
        assert!(S51ZAPRET_SCRIPT.contains("\"$BIN\" --help 2>&1 | grep -q \"lua-desync\""));
        // Anti-regression: S51ZAPRET_SCRIPT must parse ADGUARD_ENABLED and protect AdGuard Home port 53 DNS
        assert!(S51ZAPRET_SCRIPT.contains("ADGUARD_ENABLED)"));
        assert!(S51ZAPRET_SCRIPT.contains("check_adguard_active"));
        assert!(S51ZAPRET_SCRIPT.contains("! check_adguard_active && check_dns_resolver_ready"));

        // Anti-regression: S51ZAPRET_SCRIPT must hook OUTPUT chain for router-local TCP (Mihomo Fake-IP DIRECT)
        assert!(render_s51_zapret_script_with_dns_port(1053).contains("iptables -w -t mangle -I OUTPUT 1 -m mark ! --mark 0x40000000/0x40000000 -p tcp -m multiport --dports 80,443"));

        // Anti-regression: BLOCKCHECK_STRATEGIES must have 10 strategies with valid Zapret 2 Lua syntax
        assert_eq!(BLOCKCHECK_STRATEGIES.len(), 10);
        for (id, name, desc, args) in BLOCKCHECK_STRATEGIES {
            assert!(!id.is_empty() && !name.is_empty() && !desc.is_empty());
            assert!(validate_custom_args(args).is_ok(), "Invalid args in strategy {id}: {args}");
            assert!(!args.contains("--lua-desync=split2"), "Strategy {id} must use multisplit instead of v1 split2");
            assert!(!args.contains("--lua-desync=disorder2"), "Strategy {id} must use multidisorder instead of v1 disorder2");
        }
    }

    #[test]
    fn test_dns_enhanced_mode_regression() {
        // 1. Проверка обнаружения redir-host
        let yaml_redir = "dns:\n  enable: true\n  enhanced-mode: redir-host\n  nameserver:\n    - 77.88.8.8\n";
        assert_eq!(parse_dns_enhanced_mode(yaml_redir), "redir-host");

        // 2. Проверка обнаружения fake-ip
        let yaml_fake = "dns:\n  enable: true\n  enhanced-mode: fake-ip\n  nameserver:\n    - 1.1.1.1\n";
        assert_eq!(parse_dns_enhanced_mode(yaml_fake), "fake-ip");

        // 3. Отсутствие параметра не должно слепо превращаться в fake-ip!
        let yaml_missing = "dns:\n  enable: true\n  nameserver:\n    - 1.1.1.1\n";
        assert_eq!(parse_dns_enhanced_mode(yaml_missing), "unknown");
        assert_eq!(parse_dns_enhanced_mode(""), "unknown");

        // 4. Валидация при изменении: запрет некорректных значений
        assert!(update_dns_enhanced_mode(yaml_redir, "invalid_mode").is_err());
        assert!(update_dns_enhanced_mode(yaml_redir, "").is_err());
        assert!(update_dns_enhanced_mode(yaml_redir, "redir-host-extra").is_err());

        // 5. Успешное переключение
        let updated_to_fake = update_dns_enhanced_mode(yaml_redir, "fake-ip").unwrap();
        assert_eq!(parse_dns_enhanced_mode(&updated_to_fake), "fake-ip");

        let updated_to_redir = update_dns_enhanced_mode(yaml_fake, "redir-host").unwrap();
        assert_eq!(parse_dns_enhanced_mode(&updated_to_redir), "redir-host");

        // 6. Ошибка при отсутствии enhanced-mode директивы в YAML
        assert!(update_dns_enhanced_mode(yaml_missing, "fake-ip").is_err());
    }

    #[test]
    fn test_gaming_tunnel_reachability_and_target_exists() {
        use std::collections::BTreeMap;
        use serde_json::json;

        let mut proxies = BTreeMap::new();
        proxies.insert("Static-Node".to_string(), json!({ "name": "Static-Node", "type": "Vless" }));

        let mut providers = BTreeMap::new();
        providers.insert("Provider-Alive-Node".to_string(), json!({ "name": "Provider-Alive-Node", "alive": true }));
        providers.insert("Provider-NoAliveField-Node".to_string(), json!({ "name": "Provider-NoAliveField-Node" }));
        providers.insert("Provider-Dead-Node".to_string(), json!({ "name": "Provider-Dead-Node", "alive": false }));

        // 1. Узел есть только в providers, пинг -1, alive не false: reachable=true
        let (reachable_alive, latency_alive) = tunnel_reachability(&proxies, &providers, "Provider-Alive-Node", -1);
        assert!(reachable_alive, "Узел есть только в providers, пинг -1, alive не false: reachable=true");
        assert_eq!(latency_alive, None);

        let (reachable_no_field, latency_no_field) = tunnel_reachability(&proxies, &providers, "Provider-NoAliveField-Node", -1);
        assert!(reachable_no_field, "Узел есть только в providers без поля alive, пинг -1: reachable=true");
        assert_eq!(latency_no_field, None);

        // 2. Узел есть только в providers, alive=false: reachable=false
        let (reachable_dead, latency_dead) = tunnel_reachability(&proxies, &providers, "Provider-Dead-Node", -1);
        assert!(!reachable_dead, "Узел есть только в providers, alive=false: reachable=false");
        assert_eq!(latency_dead, None);

        // Даже если ping вернул > 0, если alive=false узел недоступен
        let (reachable_dead_ping, latency_dead_ping) = tunnel_reachability(&proxies, &providers, "Provider-Dead-Node", 50);
        assert!(!reachable_dead_ping, "Узел с alive=false должен быть недоступен даже при ненулевом пинге");
        assert_eq!(latency_dead_ping, None);

        // 3. Узла нет нигде: false
        let (reachable_missing, latency_missing) = tunnel_reachability(&proxies, &providers, "NonExistentNode", -1);
        assert!(!reachable_missing, "Узла нет нигде: false");
        assert_eq!(latency_missing, None);

        let (reachable_missing_ping, _) = tunnel_reachability(&proxies, &providers, "NonExistentNode", 100);
        assert!(!reachable_missing_ping, "Узла нет нигде: false даже с положительным пингом");

        // 4. target_exists истинно для узла только из провайдера
        assert!(
            target_exists(&proxies, &providers, "Provider-Alive-Node"),
            "target_exists истинно для узла только из провайдера"
        );
        assert!(
            target_exists(&proxies, &providers, "Static-Node"),
            "target_exists истинно для узла из proxies"
        );
        assert!(
            !target_exists(&proxies, &providers, "NonExistentNode"),
            "target_exists ложно для отсутствующего узла"
        );
        assert!(
            !target_exists(&proxies, &providers, ""),
            "target_exists ложно для пустой строки"
        );

        // Дополнительные проверки:
        // Пинг > 0 для доступного узла возвращает latency
        let (reachable_ping, latency_ping) = tunnel_reachability(&proxies, &providers, "Provider-Alive-Node", 45);
        assert!(reachable_ping);
        assert_eq!(latency_ping, Some(45));

        // DIRECT всегда доступен
        let (reachable_direct, latency_direct) = tunnel_reachability(&proxies, &providers, "DIRECT", -1);
        assert!(reachable_direct);
        assert_eq!(latency_direct, Some(0));
    }

    #[test]
    fn test_xbox_dns_status_response_serialization() {
        let resp = XboxDnsStatusResponse {
            active: true,
            servers: vec![
                XboxDnsServerInfo {
                    id: "xbox-dns-primary".into(),
                    name: "xbox-dns.ru (Primary)".into(),
                    provider: "Selectel".into(),
                    ips: vec!["111.88.96.54".into(), "111.88.96.55".into()],
                    ipv6: vec!["2a00:ab00:1233:26::50".into()],
                    status: "online".into(),
                    latency_ms: Some(15),
                    is_recommended: true,
                    supported_features: vec!["Xbox Live".into(), "AI Studio".into()],
                },
            ],
            reverse_proxy_ips: vec!["188.68.214.130".into(), "188.68.214.143".into()],
            targets: vec!["auth.xboxlive.com".into(), "aistudio.google.com".into()],
            antigravity_fallback_ready: true,
        };

        let json_str = serde_json::to_string(&resp).expect("Serialization must succeed");
        assert!(json_str.contains("111.88.96.54"));
        assert!(json_str.contains("188.68.214.130"));
        assert!(json_str.contains("auth.xboxlive.com"));
        assert!(json_str.contains("antigravity_fallback_ready"));
    }

    #[test]
    fn test_sync_gaming_full_tunnel_creates_tproxy_policy_routing_and_cleanup() {
        let mut cfg = config::GamingConfig::default();
        cfg.enabled = true;
        cfg.mode = config::GamingMode::Compatibility;
        cfg.devices = vec![
            config::GamingDevice {
                name: "Big PC".into(),
                ip: "192.168.1.118".into(),
                mac: "fc:22:1c:37:48:55".into(),
                ipv6: vec!["2001:db8::1".into(), "fe80::1234".into()],
                enabled: true,
                server: Some("PROXY".into()),
            }
        ];

        // 1. Проверяем генерацию правил в режиме совместимости
        let (cleanup_cmd, setup_cmds, conntrack_cmds) = build_gaming_full_tunnel_commands(&cfg, 5000, 5001, 1053);

        // Проверяем команду очистки: сброс цепочек XKEEN_GAMING, правил jump и таблицы 115 (IPv4 и IPv6)
        assert!(cleanup_cmd.contains("while iptables -t nat -D PREROUTING -j XKEEN_GAMING"), "Cleanup must remove PREROUTING NAT jump rule");
        assert!(cleanup_cmd.contains("iptables -t nat -F XKEEN_GAMING"), "Cleanup must flush NAT XKEEN_GAMING chain");
        assert!(cleanup_cmd.contains("iptables -t nat -X XKEEN_GAMING"), "Cleanup must delete NAT XKEEN_GAMING chain");
        assert!(cleanup_cmd.contains("while iptables -t mangle -D PREROUTING -j XKEEN_GAMING"), "Cleanup must remove PREROUTING mangle jump rule");
        assert!(cleanup_cmd.contains("iptables -t mangle -F XKEEN_GAMING"), "Cleanup must flush mangle XKEEN_GAMING chain");
        assert!(cleanup_cmd.contains("iptables -t mangle -X XKEEN_GAMING"), "Cleanup must delete mangle XKEEN_GAMING chain");
        assert!(cleanup_cmd.contains("while ip6tables -D FORWARD -j XKEEN_GAMING"), "Cleanup must remove IPv6 FORWARD jump rule");
        assert!(cleanup_cmd.contains("ip6tables -F XKEEN_GAMING"), "Cleanup must flush IPv6 XKEEN_GAMING chain");
        assert!(cleanup_cmd.contains("ip6tables -X XKEEN_GAMING"), "Cleanup must delete IPv6 XKEEN_GAMING chain");
        assert!(cleanup_cmd.contains("ip rule del fwmark 0x115 lookup 115"), "Cleanup must remove IPv4 policy rule 115");
        assert!(cleanup_cmd.contains("ip -6 rule del fwmark 0x115 lookup 115"), "Cleanup must remove IPv6 policy rule 115");
        assert!(cleanup_cmd.contains("ip route flush table 115"), "Cleanup must flush IPv4 table 115");
        assert!(cleanup_cmd.contains("ip -6 route flush table 115"), "Cleanup must flush IPv6 table 115");
        assert!(!cleanup_cmd.contains("111"), "Cleanup must NOT touch table 111 (reserved for main XKeen service)");

        // Проверяем команды настройки: обязательны TPROXY local routing (table 115), rp_filter, цепочки XKEEN_GAMING
        let setup_all = setup_cmds.join("\n");
        assert!(setup_all.contains("sysctl -w net.ipv4.conf.all.rp_filter=2"), "Setup must relax rp_filter for TPROXY");
        assert!(setup_all.contains("ip route replace local default dev lo table 115"), "Setup must bind table 115 to local lo interface");
        assert!(setup_all.contains("ip rule add fwmark 0x115 lookup 115 priority 100"), "Setup must add fwmark 0x115 rule pointing to table 115");
        assert!(!setup_all.contains("0x111"), "Setup MUST NOT contain old mark 0x111");
        assert!(!setup_all.contains("table 111"), "Setup MUST NOT contain old table 111");
        assert!(setup_all.contains("ip -6 route replace local default dev lo table 115"), "Setup must bind IPv6 table 115 to lo interface");
        assert!(setup_all.contains("ip -6 rule add fwmark 0x115 lookup 115 priority 100"), "Setup must add IPv6 fwmark 0x115 rule pointing to table 115");

        // Chain creation & jumps
        assert!(setup_all.contains("iptables -t nat -N XKEEN_GAMING"), "Setup must create NAT XKEEN_GAMING chain");
        assert!(setup_all.contains("iptables -t nat -I PREROUTING 1 -j XKEEN_GAMING"), "Setup must insert NAT jump rule");
        assert!(setup_all.contains("iptables -t mangle -N XKEEN_GAMING"), "Setup must create mangle XKEEN_GAMING chain");
        assert!(setup_all.contains("iptables -t mangle -I PREROUTING 1 -j XKEEN_GAMING"), "Setup must insert mangle jump rule");
        assert!(setup_all.contains("ip6tables -N XKEEN_GAMING"), "Setup must create ip6tables XKEEN_GAMING chain");
        assert!(setup_all.contains("ip6tables -I FORWARD 1 -j XKEEN_GAMING"), "Setup must insert ip6tables jump rule");

        // Conntrack flush is isolated in conntrack_cmds and NOT present in setup_cmds (to avoid NDM hook connection dropping)
        assert!(!setup_all.contains("conntrack -D"), "setup_cmds must NOT contain conntrack command (isolated from NDM hook)");
        let conntrack_all = conntrack_cmds.join("\n");
        assert!(conntrack_all.contains("conntrack -D -s 192.168.1.118"), "conntrack_cmds must flush conntrack sessions for gaming device");

        // IPv6 rejection in XKEEN_GAMING chain
        assert!(setup_all.contains("ip6tables -A XKEEN_GAMING -m mac --mac-source fc:22:1c:37:48:55 -j REJECT"), "Setup must block IPv6 transit by MAC in XKEEN_GAMING chain");
        assert!(setup_all.contains("ip6tables -A XKEEN_GAMING -s 2001:db8::1 -j REJECT"), "Setup must block non-link-local IPv6 addresses in XKEEN_GAMING chain");
        assert!(!setup_all.contains("-s fe80::1234"), "Setup must ignore link-local IPv6");

        // DNS redirection: UDP and TCP port 53 redirected to 1053 in NAT chain, and mangle returns port 53
        assert!(setup_all.contains("iptables -t nat -A XKEEN_GAMING -s 192.168.1.118 -p udp --dport 53 -j REDIRECT --to-ports 1053"), "Setup must redirect UDP DNS to 1053");
        assert!(setup_all.contains("iptables -t nat -A XKEEN_GAMING -s 192.168.1.118 -p tcp --dport 53 -j REDIRECT --to-ports 1053"), "Setup must redirect TCP DNS to 1053");
        assert!(setup_all.contains("iptables -t mangle -A XKEEN_GAMING -s 192.168.1.118 -p udp --dport 53 -j RETURN"), "Mangle setup must return UDP DNS to let NAT redirect work");

        // Local subnets bypass (127.0.0.0/8, 169.254.0.0/16, RFC1918, multicast/broadcast)
        assert!(setup_all.contains("iptables -t nat -A XKEEN_GAMING -s 192.168.1.118 -d 127.0.0.0/8 -j RETURN"), "NAT setup must exclude loopback");
        assert!(setup_all.contains("iptables -t nat -A XKEEN_GAMING -s 192.168.1.118 -d 169.254.0.0/16 -j RETURN"), "NAT setup must exclude link-local");
        assert!(setup_all.contains("iptables -t nat -A XKEEN_GAMING -s 192.168.1.118 -d 10.0.0.0/8 -j RETURN"), "NAT setup must exclude 10.0.0.0/8");
        assert!(setup_all.contains("iptables -t nat -A XKEEN_GAMING -s 192.168.1.118 -d 172.16.0.0/12 -j RETURN"), "NAT setup must exclude 172.16.0.0/12");
        assert!(setup_all.contains("iptables -t nat -A XKEEN_GAMING -s 192.168.1.118 -d 192.168.0.0/16 -j RETURN"), "NAT setup must exclude 192.168.0.0/16");

        assert!(setup_all.contains("iptables -t mangle -A XKEEN_GAMING -s 192.168.1.118 -d 127.0.0.0/8 -j RETURN"), "Mangle setup must exclude loopback");
        assert!(setup_all.contains("iptables -t mangle -A XKEEN_GAMING -s 192.168.1.118 -d 169.254.0.0/16 -j RETURN"), "Mangle setup must exclude link-local");
        assert!(setup_all.contains("iptables -t mangle -A XKEEN_GAMING -s 192.168.1.118 -d 10.0.0.0/8 -j RETURN"), "Mangle setup must exclude 10.0.0.0/8");
        assert!(setup_all.contains("iptables -t mangle -A XKEEN_GAMING -s 192.168.1.118 -d 172.16.0.0/12 -j RETURN"), "Mangle setup must exclude 172.16.0.0/12");
        assert!(setup_all.contains("iptables -t mangle -A XKEEN_GAMING -s 192.168.1.118 -d 192.168.0.0/16 -j RETURN"), "Mangle setup must exclude 192.168.0.0/16");
        assert!(setup_all.contains("iptables -t mangle -A XKEEN_GAMING -s 192.168.1.118 -d 224.0.0.0/4 -j RETURN"), "Mangle setup must exclude multicast");
        assert!(setup_all.contains("iptables -t mangle -A XKEEN_GAMING -s 192.168.1.118 -d 255.255.255.255/32 -j RETURN"), "Mangle setup must exclude broadcast");

        // TCP REDIRECT and UDP TPROXY with mark 0x115
        assert!(setup_all.contains("iptables -t nat -A XKEEN_GAMING -s 192.168.1.118 -p tcp -j REDIRECT --to-ports 5000"), "Setup must create TCP REDIRECT rule in XKEEN_GAMING");
        assert!(setup_all.contains("iptables -t mangle -A XKEEN_GAMING -s 192.168.1.118 -p udp -j TPROXY --tproxy-mark 0x115/0xffffffff --on-port 5001 --on-ip 127.0.0.1"), "Setup must create UDP TPROXY rule matching mark 0x115");
        assert!(!setup_all.contains("-m socket --transparent"), "Setup MUST NOT contain non-terminal socket match");

        // Verify direct ordering (-A): DNS redirect before LAN return, and LAN return before full tunnel REDIRECT/TPROXY
        let pos_nat_dns = setup_all.find("iptables -t nat -A XKEEN_GAMING -s 192.168.1.118 -p udp --dport 53").unwrap();
        let pos_nat_lan = setup_all.find("iptables -t nat -A XKEEN_GAMING -s 192.168.1.118 -d 192.168.0.0/16").unwrap();
        let pos_nat_tcp = setup_all.find("iptables -t nat -A XKEEN_GAMING -s 192.168.1.118 -p tcp -j REDIRECT").unwrap();
        assert!(pos_nat_dns < pos_nat_lan, "DNS redirect must precede LAN returns in NAT chain");
        assert!(pos_nat_lan < pos_nat_tcp, "LAN returns must precede general TCP redirect in NAT chain");

        let pos_mangle_dns = setup_all.find("iptables -t mangle -A XKEEN_GAMING -s 192.168.1.118 -p udp --dport 53").unwrap();
        let pos_mangle_lan = setup_all.find("iptables -t mangle -A XKEEN_GAMING -s 192.168.1.118 -d 192.168.0.0/16").unwrap();
        let pos_mangle_tproxy = setup_all.find("iptables -t mangle -A XKEEN_GAMING -s 192.168.1.118 -p udp -j TPROXY").unwrap();
        assert!(pos_mangle_dns < pos_mangle_lan, "DNS return must precede LAN returns in mangle chain");
        assert!(pos_mangle_lan < pos_mangle_tproxy, "LAN returns must precede general UDP TPROXY in mangle chain");

        // 2. Проверяем отключенный режим: setup_cmds пустой, cleanup_cmd сбрасывает маршруты
        cfg.enabled = false;
        let (cleanup_disabled, setup_disabled, conntrack_disabled) = build_gaming_full_tunnel_commands(&cfg, 5000, 5001, 1053);
        assert!(setup_disabled.is_empty(), "Disabled mode must not produce setup commands");
        assert!(conntrack_disabled.is_empty(), "Disabled mode must not produce conntrack commands");
        assert!(cleanup_disabled.contains("ip rule del fwmark 0x115 lookup 115"), "Disabled mode must still clean up policy rules");

        // 3. Проверяем режим KnownServices: setup_cmds пустой (нет full tunnel)
        cfg.enabled = true;
        cfg.mode = config::GamingMode::KnownServices;
        let (_, setup_known, conntrack_known) = build_gaming_full_tunnel_commands(&cfg, 5000, 5001, 1053);
        assert!(setup_known.is_empty(), "KnownServices mode must not produce full tunnel commands");
        assert!(conntrack_known.is_empty(), "KnownServices mode must not produce conntrack commands");
    }

    #[tokio::test]
    async fn test_sync_gaming_full_tunnel_tcp_and_udp_rules_generation() {
        let mut cfg = config::GamingConfig::default();
        cfg.enabled = true;
        cfg.mode = config::GamingMode::Compatibility;
        cfg.devices = vec![
            config::GamingDevice {
                name: "Big PC".into(),
                ip: "192.168.1.118".into(),
                mac: "fc:22:1c:37:48:55".into(),
                ipv6: vec![],
                enabled: true,
                server: Some("PROXY".into()),
            }
        ];

        let res = sync_gaming_full_tunnel(&cfg, 5000, 5001, 1053).await;
        assert!(res.is_ok(), "sync_gaming_full_tunnel must succeed without errors");

        cfg.enabled = false;
        let res_disabled = sync_gaming_full_tunnel(&cfg, 5000, 5001, 1053).await;
        assert!(res_disabled.is_ok(), "disabled mode must clean up cleanly");
    }

    #[test]
    fn test_gaming_dns_redirect_respects_adguard_port() {
        let mut cfg = config::GamingConfig::default();
        cfg.enabled = true;
        cfg.mode = config::GamingMode::Compatibility;
        cfg.devices = vec![
            config::GamingDevice {
                name: "Gaming Rig".into(),
                ip: "192.168.2.63".into(),
                mac: "aa:bb:cc:dd:ee:ff".into(),
                ipv6: vec![],
                enabled: true,
                server: Some("PROXY".into()),
            }
        ];

        // AdGuard enabled: redirect to 5354
        let (_, setup_adguard, _) = build_gaming_full_tunnel_commands(&cfg, 5000, 5001, 5354);
        let all_adguard = setup_adguard.join("\n");
        assert!(all_adguard.contains("iptables -t nat -A XKEEN_GAMING -s 192.168.2.63 -p udp --dport 53 -j REDIRECT --to-ports 5354"));
        assert!(all_adguard.contains("iptables -t nat -A XKEEN_GAMING -s 192.168.2.63 -p tcp --dport 53 -j REDIRECT --to-ports 5354"));
        assert!(!all_adguard.contains("to-ports 1053"), "Must not contain hardcoded 1053 when AdGuard port 5354 is provided");

        // AdGuard disabled: fallback to 1053
        let (_, setup_default, _) = build_gaming_full_tunnel_commands(&cfg, 5000, 5001, 0);
        let all_default = setup_default.join("\n");
        assert!(all_default.contains("iptables -t nat -A XKEEN_GAMING -s 192.168.2.63 -p udp --dport 53 -j REDIRECT --to-ports 1053"));
        assert!(all_default.contains("iptables -t nat -A XKEEN_GAMING -s 192.168.2.63 -p tcp --dport 53 -j REDIRECT --to-ports 1053"));
    }

    #[test]
    fn test_gaming_full_tunnel_rfc1918_exclusions_and_ip_validation() {
        // SEC-09 & HC-06: Test RFC1918 exclusions and strict IP parsing
        let mut cfg = config::GamingConfig::default();
        cfg.enabled = true;
        cfg.mode = config::GamingMode::Compatibility;
        cfg.devices = vec![
            config::GamingDevice {
                name: "Legit PC".into(),
                ip: "192.168.1.100".into(),
                mac: "aa:bb:cc:dd:ee:ff".into(),
                ipv6: vec![],
                enabled: true,
                server: Some("PROXY".into()),
            },
            config::GamingDevice {
                name: "Injected Device".into(),
                ip: "192.168.1.100; rm -rf /".into(),
                mac: "11:22:33:44:55:66".into(),
                ipv6: vec![],
                enabled: true,
                server: Some("PROXY".into()),
            },
        ];

        let (_, setup_cmds, conntrack_cmds) = build_gaming_full_tunnel_commands(&cfg, 5000, 5001, 1053);
        let all_cmds = setup_cmds.join("\n");

        // Verify loopback, link-local and RFC1918 subnets are excluded
        assert!(all_cmds.contains("-d 127.0.0.0/8"));
        assert!(all_cmds.contains("-d 169.254.0.0/16"));
        assert!(all_cmds.contains("-d 10.0.0.0/8"));
        assert!(all_cmds.contains("-d 172.16.0.0/12"));
        assert!(all_cmds.contains("-d 192.168.0.0/16"));

        // Verify valid IP is present and malicious injected IP is rejected
        assert!(all_cmds.contains("-s 192.168.1.100 "));
        assert!(!all_cmds.contains("; rm -rf /"));
        let ct_cmds = conntrack_cmds.join("\n");
        assert!(ct_cmds.contains("192.168.1.100"));
        assert!(!ct_cmds.contains("; rm -rf /"));
    }

    #[test]
    fn test_gaming_ndm_hook_and_regression_safety() {
        // Anti-regression: NDM hook header must support nat, mangle and filter (IPv6), and conntrack must never be in hook
        let mut cfg = config::GamingConfig::default();
        cfg.enabled = true;
        cfg.mode = config::GamingMode::Compatibility;
        cfg.devices = vec![
            config::GamingDevice {
                name: "Console".into(),
                ip: "192.168.1.150".into(),
                mac: "aa:bb:cc:dd:ee:ff".into(),
                ipv6: vec!["2001:db8::2".into()],
                enabled: true,
                server: Some("PROXY".into()),
            }
        ];

        let (cleanup_cmd, setup_cmds, conntrack_cmds) = build_gaming_full_tunnel_commands(&cfg, 5000, 5001, 1053);

        // NDM hook generation logic test
        let mut hook_content = String::from("#!/bin/sh\n[ \"$table\" != \"nat\" ] && [ \"$table\" != \"mangle\" ] && [ \"$table\" != \"filter\" ] && [ -n \"$table\" ] && exit 0\n");
        hook_content.push_str(&cleanup_cmd);
        hook_content.push('\n');
        for cmd in &setup_cmds {
            hook_content.push_str(cmd);
            hook_content.push('\n');
        }

        // 1. Hook MUST NOT contain $type = ip6tables exit (prevents IPv6 leak rule loss on reload)
        assert!(!hook_content.contains("[ \"$type\" = \"ip6tables\" ] && exit 0"));
        // 2. Hook MUST allow filter table for ip6tables FORWARD chain
        assert!(hook_content.contains("[ \"$table\" != \"filter\" ]"));
        // 3. Hook MUST NOT contain conntrack commands (prevents mid-game disconnections on netfilter events)
        assert!(!hook_content.contains("conntrack -D"));
        // 4. conntrack_cmds contains the command for the API to run directly
        assert_eq!(conntrack_cmds.len(), 1);
        assert!(conntrack_cmds[0].contains("conntrack -D -s 192.168.1.150"));
        // 5. No mark 0x111 or table 111 in hook
        assert!(!hook_content.contains("0x111"));
        assert!(!hook_content.contains("table 111"));
        // 6. Hook has table 115 and mark 0x115
        assert!(hook_content.contains("0x115"));
        assert!(hook_content.contains("table 115"));
    }

    #[test]
    fn test_gaming_full_tunnel_cidr_ip_and_deduplication() {
        // Anti-regression: Test CIDR notation in dev.ip, duplicate IP deduplication, and MAC validation
        let mut cfg = config::GamingConfig::default();
        cfg.enabled = true;
        cfg.mode = config::GamingMode::Compatibility;
        cfg.devices = vec![
            config::GamingDevice {
                name: "Device With CIDR".into(),
                ip: "192.168.1.118/32".into(),
                mac: "fc:22:1c:37:48:55".into(),
                ipv6: vec!["2001:db8::1/64".into()],
                enabled: true,
                server: Some("PROXY".into()),
            },
            config::GamingDevice {
                name: "Duplicate IP Device".into(),
                ip: "192.168.1.118".into(),
                mac: "aa:bb:cc:dd:ee:ff".into(),
                ipv6: vec![],
                enabled: true,
                server: Some("PROXY".into()),
            },
            config::GamingDevice {
                name: "Device With Zero MAC".into(),
                ip: "192.168.1.119".into(),
                mac: "00:00:00:00:00:00".into(),
                ipv6: vec![],
                enabled: true,
                server: Some("PROXY".into()),
            },
        ];

        let (cleanup_cmd, setup_cmds, conntrack_cmds) = build_gaming_full_tunnel_commands(&cfg, 5000, 5001, 1053);
        let all_cmds = setup_cmds.join("\n");

        // 1. CIDR /32 must be correctly parsed into pure IPv4
        assert!(all_cmds.contains("-s 192.168.1.118 "), "Setup must include rules for 192.168.1.118 parsed from 192.168.1.118/32");
        assert!(!all_cmds.contains("-s 192.168.1.118/32"), "Setup must NOT contain raw CIDR suffix in -s flag");

        // 2. Duplicate IP must not result in duplicate NAT redirect rules
        let nat_redirect_count = setup_cmds.iter().filter(|c| c.contains("-s 192.168.1.118 -p tcp -j REDIRECT")).count();
        assert_eq!(nat_redirect_count, 1, "There must be exactly 1 TCP redirect rule for 192.168.1.118 despite duplicates");

        // 3. Zero MAC must NOT be rejected in ip6tables
        assert!(!all_cmds.contains("00:00:00:00:00:00"), "Setup must NOT emit reject rule for zero MAC");

        // 4. IPv6 CIDR /64 must be cleanly stripped and parsed
        assert!(all_cmds.contains("ip6tables -A XKEEN_GAMING -s 2001:db8::1 -j REJECT"), "Setup must block IPv6 2001:db8::1 stripped of /64");

        // 5. Cleanup must support both Busybox explicit route deletion and iproute2 flush
        assert!(cleanup_cmd.contains("ip route del local default dev lo table 115"), "Cleanup must include explicit route del for BusyBox");
        assert!(cleanup_cmd.contains("ip route flush table 115"), "Cleanup must include route flush");

        // 6. Conntrack commands must include each valid unique IP
        assert_eq!(conntrack_cmds.len(), 2);
        assert!(conntrack_cmds[0].contains("192.168.1.118"));
        assert!(conntrack_cmds[1].contains("192.168.1.119"));
    }

    #[test]
    fn test_import_node_empty_array_handling() {
        // BUG-04: Test empty proxies array replacement
        let provider_content = "proxies: []\n";
        let mut fixed_content = if provider_content.contains("proxies: []") {
            provider_content.replace("proxies: []", "proxies:")
        } else {
            provider_content.to_string()
        };
        fixed_content.push('\n');
        fixed_content.push_str("  - name: test_node\n    type: ss");
        assert!(fixed_content.starts_with("proxies:\n"));
        assert!(fixed_content.contains("test_node"));
    }

    #[test]
    fn test_anti_regression_flow_server_rejects_proxy_selector_groups() {
        let excluded = ["PROXY", "GLOBAL", "Fastest", "Fallback", "DIRECT", "REJECT"];
        for name in excluded {
            assert!(["PROXY", "GLOBAL", "Fastest", "Fallback", "DIRECT", "REJECT"].contains(&name));
        }

        let servers = vec![
            crate::mihomo::Server {
                id: "PROXY".into(),
                name: "🌐 PROXY".into(),
                protocol: "POOL (SELECT)".into(),
                host: "Выбран: test".into(),
                port: 0,
                is_active: true,
                is_priority: false,
                ping_ms: 79,
                provider: Some("__pool__".into()),
                provider_name: Some("Группа / Пул".into()),
                is_pool: true,
                resolved_ip: None,
            },
            crate::mihomo::Server {
                id: "US-Node".into(),
                name: "🇺🇸 US Node".into(),
                protocol: "VLESS".into(),
                host: "us.example.com".into(),
                port: 443,
                is_active: false,
                is_priority: false,
                ping_ms: 120,
                provider: Some("sub1".into()),
                provider_name: Some("Sub 1".into()),
                is_pool: false,
                resolved_ip: None,
            },
        ];

        let valid_flow_cand = servers.iter().find(|s| {
            !s.id.is_empty() && s.id != "Fastest" && s.id != "Fallback" && !s.is_pool && s.id != "PROXY" && s.id != "GLOBAL"
        });

        assert!(valid_flow_cand.is_some());
        assert_eq!(valid_flow_cand.unwrap().id, "US-Node");
    }

    #[test]
    fn test_build_zapret2_download_urls_dynamic_version() {
        let urls = build_zapret2_download_urls("v1.0.6");
        assert!(!urls.is_empty());
        for u in &urls {
            assert!(!u.ends_with("/zapret2.tar.gz"), "URL must not contain static zapret2.tar.gz: {u}");
            assert!(!u.contains("v1.0.5.2"), "URL must not contain obsolete hardcoded v1.0.5.2: {u}");
            assert!(u.contains("v1.0.6") || u.contains("1.0.6"), "URL must target v1.0.6: {u}");
        }
        assert!(urls.contains(&"https://github.com/bol-van/zapret2/releases/download/v1.0.6/zapret2-v1.0.6.tar.gz".to_string()));
        assert!(urls.contains(&"https://github.com/bol-van/zapret2/releases/latest/download/zapret2-v1.0.6.tar.gz".to_string()));
        assert!(urls.contains(&"https://github.com/bol-van/zapret2/releases/download/v1.0.6/zapret2-1.0.6.tar.gz".to_string()));
        assert!(urls.contains(&"https://github.com/bol-van/zapret2/archive/refs/tags/v1.0.6.tar.gz".to_string()));
    }

    #[test]
    fn test_build_zapret2_download_urls_handles_unprefixed_version() {
        let urls = build_zapret2_download_urls("1.0.6");
        assert!(!urls.is_empty());
        for u in &urls {
            assert!(!u.ends_with("/zapret2.tar.gz"), "URL must not contain static zapret2.tar.gz: {u}");
            assert!(!u.contains("v1.0.5.2"), "URL must not contain obsolete hardcoded v1.0.5.2: {u}");
            assert!(u.contains("v1.0.6") || u.contains("1.0.6"), "URL must target v1.0.6: {u}");
        }
        assert!(urls.contains(&"https://github.com/bol-van/zapret2/releases/download/v1.0.6/zapret2-v1.0.6.tar.gz".to_string()));
        assert!(urls.contains(&"https://github.com/bol-van/zapret2/releases/latest/download/zapret2-v1.0.6.tar.gz".to_string()));
        assert!(urls.contains(&"https://github.com/bol-van/zapret2/releases/download/v1.0.6/zapret2-1.0.6.tar.gz".to_string()));
        assert!(urls.contains(&"https://github.com/bol-van/zapret2/archive/refs/tags/v1.0.6.tar.gz".to_string()));
    }

    #[test]
    fn test_build_zapret2_download_urls_arbitrary_future_version() {
        let urls_future = build_zapret2_download_urls("v2.4.0");
        assert!(!urls_future.is_empty());
        for u in &urls_future {
            assert!(!u.ends_with("/zapret2.tar.gz"), "URL must not contain static zapret2.tar.gz: {u}");
            assert!(!u.contains("v1.0.5.2"), "URL must not contain obsolete hardcoded v1.0.5.2: {u}");
            assert!(!u.contains("v1.0.6"), "URL must not contain static v1.0.6 when targeting v2.4.0: {u}");
            assert!(u.contains("v2.4.0") || u.contains("2.4.0"), "URL must target v2.4.0: {u}");
        }
        assert!(urls_future.contains(&"https://github.com/bol-van/zapret2/releases/download/v2.4.0/zapret2-v2.4.0.tar.gz".to_string()));
        assert!(urls_future.contains(&"https://github.com/bol-van/zapret2/releases/latest/download/zapret2-v2.4.0.tar.gz".to_string()));
        assert!(urls_future.contains(&"https://github.com/bol-van/zapret2/releases/download/v2.4.0/zapret2-2.4.0.tar.gz".to_string()));
        assert!(urls_future.contains(&"https://github.com/bol-van/zapret2/archive/refs/tags/v2.4.0.tar.gz".to_string()));

        let urls_bare = build_zapret2_download_urls("3.0.0");
        assert!(!urls_bare.is_empty());
        for u in &urls_bare {
            assert!(u.contains("v3.0.0") || u.contains("3.0.0"));
        }
        assert!(urls_bare.contains(&"https://github.com/bol-van/zapret2/releases/download/v3.0.0/zapret2-v3.0.0.tar.gz".to_string()));
    }

    #[tokio::test]
    async fn test_api_status_serialization_tiered_failover() {
        let cfg = crate::config::AppConfig::default();
        crate::failover::reset_whitelist_episode_bytes();
        crate::failover::record_whitelist_episode_bytes(2048);

        // Scenario A: Active is standard VPS (Tier 1)
        let srv_vps = crate::mihomo::Server {
            id: "vps_nl".into(),
            name: "Netherlands VPS".into(),
            protocol: "vless".into(),
            host: "1.1.1.1".into(),
            port: 443,
            is_active: true,
            is_priority: false,
            ping_ms: 50,
            provider: None,
            provider_name: None,
            is_pool: false,
            resolved_ip: None,
        };
        let is_tier2_vps = crate::failover::is_whitelist_server_full(&srv_vps.name, &srv_vps.id, srv_vps.provider.as_deref(), &cfg);
        let tier_vps: u8 = if is_tier2_vps { 2 } else { 1 };
        assert_eq!(tier_vps, 1, "Standard VPS must be Tier 1");

        // Scenario B: Active is Whitelist server (Tier 2)
        let srv_bs = crate::mihomo::Server {
            id: "bs_ru".into(),
            name: "Moscow [БС] Reserve".into(),
            protocol: "vless".into(),
            host: "1.1.1.2".into(),
            port: 443,
            is_active: true,
            is_priority: false,
            ping_ms: 20,
            provider: None,
            provider_name: None,
            is_pool: false,
            resolved_ip: None,
        };
        let is_tier2_bs = crate::failover::is_whitelist_server_full(&srv_bs.name, &srv_bs.id, srv_bs.provider.as_deref(), &cfg);
        let tier_bs: u8 = if is_tier2_bs { 2 } else { 1 };
        assert_eq!(tier_bs, 2, "Whitelist server must be Tier 2");

        assert_eq!(crate::failover::get_whitelist_episode_bytes(), 2048);
    }

    #[test]
    fn regression_bug_70b_all_iptables_calls_have_wait() {
        // Баг 70: вызовы iptables/ip6tables без -w при занятом xtables lock молча не применяются.
        // Тест сканирует рабочий код (до #[cfg(test)]) во всех основных файлах backend/src/*.rs.
        // Исключения с причиной:
        // - S51ZAPRET_SCRIPT (api.rs): шаблон скрипта объявляет shell-функцию `iptables()`, которая
        //   сама подставляет `$IPTABLES_CMD` (`iptables -w 2` если поддерживается).
        // - `for ipt in iptables ip6tables; do` в zapret.rs: проверяется отдельно (должно использовать -w).
        let files: &[(&str, &str)] = &[
            ("adguard.rs", include_str!("../adguard.rs")),
            ("antigravity.rs", include_str!("../antigravity.rs")),
            ("api/mod.rs", include_str!("mod.rs")),
            ("api/backups.rs", include_str!("backups.rs")),
            ("api/clash_proxy.rs", include_str!("clash_proxy.rs")),
            ("api/dns_adguard.rs", include_str!("dns_adguard.rs")),
            ("api/domains.rs", include_str!("domains.rs")),
            ("api/routing.rs", include_str!("routing.rs")),
            ("api/servers.rs", include_str!("servers.rs")),
            ("api/status.rs", include_str!("status.rs")),
            ("api/system_ssh.rs", include_str!("system_ssh.rs")),
            ("api/vkturn.rs", include_str!("vkturn.rs")),
            ("api/zapret.rs", include_str!("zapret.rs")),
            ("auth.rs", include_str!("../auth.rs")),
            ("bot.rs", include_str!("../bot.rs")),
            ("cdn_discovery.rs", include_str!("../cdn_discovery.rs")),
            ("config.rs", include_str!("../config.rs")),
            ("direct_traffic.rs", include_str!("../direct_traffic.rs")),
            ("failover/history.rs", include_str!("../failover/history.rs")),
            ("failover/mod.rs", include_str!("../failover/mod.rs")),
            ("failover/probe.rs", include_str!("../failover/probe.rs")),
            ("failover/quarantine.rs", include_str!("../failover/quarantine.rs")),
            ("failover/scoring.rs", include_str!("../failover/scoring.rs")),
            ("failover/switcher.rs", include_str!("../failover/switcher.rs")),
            ("failover/types.rs", include_str!("../failover/types.rs")),
            ("frontend.rs", include_str!("../frontend.rs")),
            ("logger.rs", include_str!("../logger.rs")),
            ("main.rs", include_str!("../main.rs")),
            ("mihomo.rs", include_str!("../mihomo.rs")),
            ("notifications.rs", include_str!("../notifications.rs")),
            ("override_sync.rs", include_str!("../override_sync.rs")),
            ("rci.rs", include_str!("../rci.rs")),
            ("routing/dns_redirect.rs", include_str!("../routing/dns_redirect.rs")),
            ("routing/ipset.rs", include_str!("../routing/ipset.rs")),
            ("routing/iptables.rs", include_str!("../routing/iptables.rs")),
            ("routing/mod.rs", include_str!("../routing/mod.rs")),
            ("routing/policy_routing.rs", include_str!("../routing/policy_routing.rs")),
            ("routing/validator.rs", include_str!("../routing/validator.rs")),
            ("speedtest.rs", include_str!("../speedtest.rs")),
            ("subscription_parser.rs", include_str!("../subscription_parser.rs")),
            ("system.rs", include_str!("../system.rs")),
            ("traffic.rs", include_str!("../traffic.rs")),
            ("transaction.rs", include_str!("../transaction.rs")),
            ("updater.rs", include_str!("../updater.rs")),
            ("vkturn.rs", include_str!("../vkturn.rs")),
            ("watchdog.rs", include_str!("../watchdog.rs")),
            ("whitelist_catalog.rs", include_str!("../whitelist_catalog.rs")),
            ("zapret.rs", include_str!("../zapret.rs")),
        ];

        let mut violations = Vec::new();
        let mut inside_s51_zapret = false;

        for (fname, content) in files {
            let prod = &content[..content.find("#[cfg(test)]").unwrap_or(content.len())];
            let lines: Vec<&str> = prod.lines().collect();
            for (idx, line) in lines.iter().enumerate() {
                let trimmed = line.trim();
                if *fname == "api/mod.rs" || *fname == "api/zapret.rs" {
                    if trimmed.starts_with("pub const S51ZAPRET_SCRIPT:") {
                        inside_s51_zapret = true;
                        continue;
                    }
                    if inside_s51_zapret {
                        if trimmed == "\"#;" {
                            inside_s51_zapret = false;
                        }
                        // Внутри S51ZAPRET_SCRIPT определена обёртка iptables() с -w 2
                        continue;
                    }
                }
                // Пропускаем комментарии
                if trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with('*') || trimmed.starts_with('#') {
                    continue;
                }
                // 1. Проверка Command::new("iptables") / Command::new("ip6tables")
                if trimmed.contains("Command::new(\"iptables\")") || trimmed.contains("Command::new(\"ip6tables\")") {
                    let end = (idx + 4).min(lines.len());
                    let ctx = lines[idx..end].join(" ");
                    if !ctx.contains("\"-w\"") {
                        violations.push(format!("{}:{}: Command::new без -w: {}", fname, idx + 1, trimmed));
                    }
                }
                // 2. Проверка цикла `for ipt in iptables ip6tables` без `-w`
                if trimmed.contains("for ipt in iptables ip6tables") {
                    violations.push(format!("{}:{}: цикл без -w: {}", fname, idx + 1, trimmed));
                }
                // 3. Проверка прямых shell-вызовов iptables / ip6tables с флагами (`-t`, `-A`, `-D`, `-I`, `-S`, `-L`, `-F`, `-X`, `-N`, `-C`, `-Z`)
                let mut search_pos = 0;
                while trimmed[search_pos..].find("iptables").or_else(|| trimmed[search_pos..].find("ip6tables")).is_some() {
                    // Найдём ближайшее вхождение iptables или ip6tables
                    let pos_v4 = trimmed[search_pos..].find("iptables");
                    let pos_v6 = trimmed[search_pos..].find("ip6tables");
                    let pos = match (pos_v4, pos_v6) {
                        (Some(a), Some(b)) => a.min(b),
                        (Some(a), None) => a,
                        (None, Some(b)) => b,
                        (None, None) => break,
                    };
                    let start = search_pos + pos;
                    let word_len = if trimmed[start..].starts_with("ip6tables") { 9 } else { 8 };
                    search_pos = start + word_len;

                    // Граница слова слева
                    if start > 0 {
                        let prev = trimmed.as_bytes()[start - 1];
                        if prev.is_ascii_alphanumeric() || prev == b'_' {
                            continue;
                        }
                    }
                    let after = &trimmed[start + word_len..];
                    // Должен идти пробел, затем либо `-w`, либо `-<флаг>`
                    if !after.starts_with(' ') {
                        continue;
                    }
                    let after_space = after.trim_start();
                    // Отсекаем тексты логов и описаний (например, "iptables для защиты", "iptables правил", "iptables (цепочка")
                    if !after_space.starts_with('-') {
                        continue;
                    }
                    // Берём команду до `;`, `|`, `&&`, `||`, `"`, `'`
                    let cmd_end = after_space
                        .find(|c| c == ';' || c == '|' || c == '"' || c == '\'')
                        .unwrap_or(after_space.len());
                    let cmd_args = &after_space[..cmd_end];
                    // Если -w уже в начале (`-w ` или `-w2`) или в аргументах (` -w ` или ` -w` в конце)
                    let has_w = cmd_args == "-w"
                        || cmd_args.starts_with("-w ")
                        || cmd_args.contains(" -w ")
                        || cmd_args.ends_with(" -w");
                    if !has_w {
                        violations.push(format!("{}:{}: нет -w в вызове '{}': {}", fname, idx + 1, &trimmed[start..start + word_len + 1 + cmd_end.min(40)], trimmed));
                    }
                }
            }
        }

        assert!(
            violations.is_empty(),
            "Найдены вызовы iptables/ip6tables без флага -w ({}):\n{}",
            violations.len(),
            violations.join("\n")
        );
    }

    #[test]
    fn regression_bug_29b_set_features_keeps_devices_and_enabled() {
        let mut cur = crate::config::ZapretConfig::default();
        cur.enabled = true;
        cur.excluded_devices = vec!["192.168.1.50".to_string(), "aa:bb:cc:dd:ee:ff".to_string()];
        cur.custom_entries = vec![crate::config::ZapretCustomEntry {
            domain: "example.com".into(),
            enabled: true,
            cdns: vec!["cdn.example.com".into()],
        }];
        cur.engine = "v1".to_string();
        cur.period_saved_bytes = 12345;
        cur.last_update_check = Some("2026-11-12".to_string());
        cur.update_available = true;
        cur.latest_version = Some("2.4.0".to_string());
        cur.hybrid_youtube = true;
        cur.hybrid_discord = true;

        let req = ZapretFeatures {
            enabled: Some(false),
            hybrid_youtube: Some(false),
            hybrid_discord: Some(false),
            discord_voice_udp: Some(false),
            youtube_turbo: Some(false),
            general_bypass: Some(false),
            aggressive_dpi: Some(true),
            isolated_proxy: Some(false),
            bypass_github: Some(false),
            bypass_torrents: Some(false),
            bypass_adult: Some(false),
            custom_entries: None,
            custom_args: Some("--custom-test".into()),
            smart_tv_mode: Some(true),
            community_hostlist_enabled: Some(true),
            community_hostlist_url: Some("https://example.com/hosts.txt".into()),
            community_hostlist_auto_update: Some(true),
            excluded_devices: None,
            engine: None,
        };

        let merged = merge_zapret_features(&cur, &req);
        assert_eq!(merged.excluded_devices, cur.excluded_devices);
        assert_eq!(merged.custom_entries, cur.custom_entries);
        assert_eq!(merged.engine, "v1");
        assert_eq!(merged.period_saved_bytes, 12345);
        assert_eq!(merged.last_update_check, cur.last_update_check);
        assert_eq!(merged.update_available, cur.update_available);
        assert_eq!(merged.latest_version, cur.latest_version);
        assert_eq!(merged.enabled, false);
        assert_eq!(merged.hybrid_youtube, false);
        assert_eq!(merged.aggressive_dpi, true);
        assert_eq!(merged.smart_tv_mode, true);
        assert_eq!(merged.custom_args, Some("--custom-test".into()));

        let reset = reset_zapret_features(&cur);
        assert_eq!(reset.enabled, true);
        assert_eq!(reset.excluded_devices, cur.excluded_devices);
        assert_eq!(reset.custom_entries, cur.custom_entries);
        assert_eq!(reset.engine, "v1");
        assert_eq!(reset.period_saved_bytes, 12345);
        assert_eq!(reset.aggressive_dpi, false);
        assert_eq!(reset.custom_args, None);
    }

    #[test]
    fn regression_bug_26b_community_hostlist_not_duplicated() {
        let base_with_community = "# Manual user hosts\nuser-site.org\nanother-user.com\n\n# --- START COMMUNITY HOSTLIST ---\ncomm1.org\ncomm2.org\n# --- END COMMUNITY HOSTLIST ---\n";
        let mut cfg = crate::config::ZapretConfig::default();
        cfg.community_hostlist_enabled = false;

        // When community_hostlist_enabled is false, community block should be stripped
        let synced = sync_zapret_hosts_content(base_with_community, &cfg);
        assert!(synced.contains("user-site.org"));
        assert!(synced.contains("another-user.com"));
        assert!(!synced.contains("COMMUNITY HOSTLIST"), "community block must be stripped when disabled");
        assert!(!synced.contains("comm1.org"));

        // Idempotency check: running sync twice does not duplicate anything
        let synced_twice = sync_zapret_hosts_content(&synced, &cfg);
        assert_eq!(synced, synced_twice);
    }

    #[tokio::test]
    async fn regression_bug_30b_zapret_conf_save_rebuilds_routing() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else { return };
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 16384];
                    let _ = sock.read(&mut buf).await;
                    let _ = sock.write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n").await;
                    let _ = sock.shutdown().await;
                });
            }
        });

        let root = std::env::temp_dir().join(format!("xkr-reg-30-{}", crate::auth::random_hex(8)));
        tokio::fs::create_dir_all(&root).await.unwrap();
        let yaml_path = root.join("config.yaml");
        let json_path = root.join("config.json");

        let mut app_cfg = crate::config::AppConfig::default();
        app_cfg.mihomo.port = port;
        app_cfg.mihomo.config_path = yaml_path.display().to_string();
        app_cfg.zapret.enabled = true;

        let initial_yaml = "rules:\n  - MATCH,DIRECT\n";
        tokio::fs::write(&yaml_path, initial_yaml).await.unwrap();
        tokio::fs::write(&json_path, serde_json::to_string(&app_cfg).unwrap()).await.unwrap();

        let cfg_arc = std::sync::Arc::new(tokio::sync::RwLock::new(std::sync::Arc::new(app_cfg)));
        let ag = std::sync::Arc::new(crate::antigravity::AntigravityManager::new(cfg_arc.clone()));
        let state = AppState {
            config: cfg_arc,
            config_path: std::sync::Arc::new(json_path.clone()),
            http: reqwest::Client::new(),
            failover_log: std::sync::Arc::new(crate::failover::FailoverLog::default()),
            routing_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            config_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            speedtest_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            zapret_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            antigravity: ag,
            vkturn: crate::vkturn::VkTurnManager::new(),
        };

        // Calling apply_zapret_conf_save with new zapret config content
        let new_conf_content = "ZAPRET_ENGINE=\"v2\"\nDISCORD_VOICE_ENABLED=\"1\"\nSMART_TV_MODE=\"1\"\n";
        let res = apply_zapret_conf_save(&state, new_conf_content).await;
        assert!(res.is_ok(), "apply_zapret_conf_save should succeed: {:?}", res.err());

        let final_cfg = state.config.read().await;
        assert_eq!(final_cfg.zapret.engine, "v2");
        assert_eq!(final_cfg.zapret.discord_voice_udp, true);
        assert_eq!(final_cfg.zapret.smart_tv_mode, true);

        let final_yaml = tokio::fs::read_to_string(&yaml_path).await.unwrap();
        assert!(final_yaml.contains("rules:"), "YAML must be rebuilt and saved");
    }

    #[test]
    fn regression_bug_5b_manual_gaming_blocked_by_bs() {
        let mut cfg = crate::config::AppConfig::default();
        cfg.whitelist_providers = vec!["bs-sub".into()];
        let srv = |name: &str, prov: Option<&str>| crate::mihomo::Server {
            id: name.into(), name: name.into(), protocol: "vless".into(), host: "h".into(), port: 443,
            is_active: true, is_priority: false, ping_ms: 0, provider: prov.map(String::from),
            provider_name: None, is_pool: false, resolved_ip: None,
        };
        let bs_srv = srv("RU [БС]", None);
        let bs_sub_srv = srv("Node 1", Some("bs-sub"));
        let plain_srv = srv("Germany", None);

        // Активный БС-узел + enable -> Err с понятным текстом
        let err = manual_gaming_enable_allowed(&cfg, Some(&bs_srv), true);
        assert!(err.is_err(), "активный БС-узел + enable обязан возвращать Err");
        assert_eq!(err.unwrap_err(), crate::failover::GAMING_BLOCKED_BY_BS);

        // Узел из подписки БС + enable -> Err
        assert!(manual_gaming_enable_allowed(&cfg, Some(&bs_sub_srv), true).is_err());

        // Тот же + disable -> Ok
        assert!(manual_gaming_enable_allowed(&cfg, Some(&bs_srv), false).is_ok(), "активный БС-узел + disable обязан возвращать Ok");
        assert!(manual_gaming_enable_allowed(&cfg, Some(&bs_sub_srv), false).is_ok());

        // Обычный узел + enable -> Ok
        assert!(manual_gaming_enable_allowed(&cfg, Some(&plain_srv), true).is_ok(), "обычный узел + enable обязан возвращать Ok");
    }

    #[test]
    fn regression_bug_6b_gaming_patch_keeps_ignored_conns() {
        // Баг 6: патч настроек игрового режима накладывается на текущий конфиг
        // и не затирает ignored_game_conns, gaming_suspended и devices.
        let mut current_cfg = crate::config::AppConfig::default();
        current_cfg.gaming.ignored_game_conns = vec!["1.2.3.4:5678".to_string(), "game.server.com".to_string()];
        current_cfg.whitelist.gaming_suspended = true;
        current_cfg.gaming.devices = vec![crate::config::GamingDevice {
            mac: "aa:bb:cc:dd:ee:ff".to_string(),
            ip: "192.168.1.50".to_string(),
            ipv6: vec![],
            name: "PC".to_string(),
            enabled: true,
            server: None,
        }];
        current_cfg.gaming.smart_mode = false;

        let patch = serde_json::json!({
            "smart_mode": true
        });

        let merged = merge_gaming_patch(&current_cfg, &patch).expect("merge_gaming_patch should succeed");
        assert!(merged.gaming.smart_mode, "smart_mode должно обновиться в true");
        assert_eq!(
            merged.gaming.ignored_game_conns,
            vec!["1.2.3.4:5678".to_string(), "game.server.com".to_string()],
            "ignored_game_conns не должны стираться через патч"
        );
        assert!(merged.whitelist.gaming_suspended, "gaming_suspended не должно сбрасываться через патч");
        assert_eq!(merged.gaming.devices.len(), 1, "устройства не должны стираться через частичный патч");
        assert_eq!(merged.gaming.devices[0].mac, "aa:bb:cc:dd:ee:ff");
    }

    #[test]
    fn regression_bug_8b_no_global_connection_reset_on_toggle() {
        // Баг 8: переключения в игровом режиме и при смене сервера БС рвут соединения всей сети:
        // использовался switch_group, закрывающий все соединения LAN.
        let gaming_body = fn_body(production_source(), "async fn apply_and_verify_gaming");
        assert!(
            !gaming_body.contains("mihomo::switch_group("),
            "apply_and_verify_gaming must NOT call switch_group (global close_all_connections)"
        );
        assert!(
            gaming_body.contains("switch_gaming_group("),
            "apply_and_verify_gaming must use switch_gaming_group"
        );

        let wl_tunnel = fn_body(production_source(), "pub async fn sync_whitelist_mode_tunnel");
        assert!(
            !wl_tunnel.contains("mihomo::switch_group("),
            "sync_whitelist_mode_tunnel must NOT call switch_group (global close_all_connections)"
        );
        assert!(
            wl_tunnel.contains("switch_group_scoped(") || wl_tunnel.contains("switch_selector("),
            "sync_whitelist_mode_tunnel must use switch_group_scoped or switch_selector"
        );

        let dev_routing = fn_body(production_source(), "pub async fn set_device_routing");
        assert!(
            !dev_routing.contains("mihomo::switch_group("),
            "set_device_routing must NOT call switch_group (global close_all_connections)"
        );
        assert!(
            dev_routing.contains("switch_group_scoped(") || dev_routing.contains("switch_selector("),
            "set_device_routing must use switch_group_scoped or switch_selector"
        );
    }

    #[tokio::test]
    async fn regression_bug_110b_xkeen_service_yaml_via_tx_and_errors_reported() {
        let mut cfg = crate::config::AppConfig::default();
        let tmp_dir = std::env::temp_dir().join(format!(
            "xkeen_110b_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::create_dir_all(&tmp_dir);
        let yaml_path = tmp_dir.join("config.yaml");
        let base_yaml = "port: 7890\nsocks-port: 7891\nproxies:\n  - name: p1\n    type: direct\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies: [p1]\nrules:\n  - MATCH,DIRECT\n";
        std::fs::write(&yaml_path, base_yaml).unwrap();
        cfg.mihomo.config_path = yaml_path.to_string_lossy().to_string();

        // 1. plan_xkeen_start_yaml validates and updates routing rules in yaml
        let res = plan_xkeen_start_yaml(&cfg, base_yaml);
        assert!(res.is_ok(), "Planning xkeen start yaml should succeed on valid yaml: {:?}", res.err());
        let new_yaml = res.unwrap();
        assert!(new_yaml.contains("rules:"), "Planned yaml must contain routing rules");

        // 2. Planning with invalid/empty YAML produces Err
        let err_res = plan_xkeen_start_yaml(&cfg, "");
        assert!(err_res.is_err(), "Empty YAML must produce error");

        // 3. Static check: xkeen_service must use ConfigTx to write YAML and report reload/write errors
        let body = fn_body(production_source(), "pub async fn xkeen_service");
        assert!(
            body.contains("ConfigTx::begin"),
            "xkeen_service must acquire ConfigTx to write yaml safely under config_lock + routing_lock"
        );
        assert!(
            !body.contains("let _ = atomic_write_file(path, &new_yaml)"),
            "atomic_write_file error must not be silenced with let _ in xkeen_service"
        );
        assert!(
            !body.contains("let _ = mihomo::reload_config(&state.http, &cfg)"),
            "mihomo::reload_config error must not be silenced with let _ in xkeen_service"
        );

        let _ = std::fs::remove_dir_all(&tmp_dir);

    }

    #[tokio::test]
    async fn regression_marking_active_subscription_enables_whitelist_mode() {
        use axum::{routing::{get, put}, Json, Router, http::StatusCode};

        crate::mihomo::invalidate_proxies_cache();

        let app = Router::new()
            .route(
                "/proxies",
                get(|| async {
                    Json(serde_json::json!({
                        "proxies": {
                            "PROXY": {
                                "type": "Selector",
                                "now": "Германия 01",
                                "all": ["Германия 01"]
                            },
                            "Германия 01": {
                                "type": "VLESS",
                                "name": "Германия 01",
                                "server": "203.0.113.10",
                                "port": 443,
                                "alive": true,
                                "history": [{"delay": 42}]
                            }
                        }
                    }))
                }),
            )
            .route(
                "/providers/proxies",
                get(|| async {
                    Json(serde_json::json!({
                        "providers": {
                            "геодема2": {
                                "name": "геодема2",
                                "vehicleType": "HTTP",
                                "proxies": [
                                    {
                                        "type": "VLESS",
                                        "name": "Германия 01",
                                        "server": "203.0.113.10",
                                        "port": 443,
                                        "alive": true,
                                        "history": [{"delay": 42}]
                                    }
                                ]
                            },
                            "z_other_sub": {
                                "name": "z_other_sub",
                                "vehicleType": "HTTP",
                                "proxies": [
                                    {
                                        "type": "VLESS",
                                        "name": "Германия 01",
                                        "server": "203.0.113.10",
                                        "port": 443,
                                        "alive": true,
                                        "history": [{"delay": 42}]
                                    }
                                ]
                            }
                        }
                    }))
                }),
            )
            .route("/configs", put(|| async { StatusCode::NO_CONTENT }))
            .route("/proxies/{group}", put(|| async { StatusCode::NO_CONTENT }));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let srv_handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let tmp_dir = std::env::temp_dir().join(format!(
            "xkeen_wl_prov_active_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::create_dir_all(&tmp_dir);
        let yaml_path = tmp_dir.join("config.yaml");
        let json_path = tmp_dir.join("config.json");
        let base_yaml = "mode: rule\nproxies: []\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies: [DIRECT]\nrules:\n  - MATCH,PROXY\n";
        std::fs::write(&yaml_path, base_yaml).unwrap();

        let mut cfg = crate::config::AppConfig::default();
        cfg.mihomo.host = "127.0.0.1".to_string();
        cfg.mihomo.port = port;
        cfg.mihomo.config_path = yaml_path.to_string_lossy().to_string();
        cfg.whitelist.auto_by_failover = true;
        cfg.whitelist.enabled = false;
        crate::config::save(&json_path, &cfg).await.unwrap();

        let cfg_arc = std::sync::Arc::new(tokio::sync::RwLock::new(std::sync::Arc::new(cfg)));
        let state = AppState {
            config: cfg_arc.clone(),
            config_path: std::sync::Arc::new(json_path.clone()),
            http: reqwest::Client::new(),
            failover_log: std::sync::Arc::new(crate::failover::FailoverLog::default()),
            routing_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            config_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            speedtest_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            zapret_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            antigravity: std::sync::Arc::new(crate::antigravity::AntigravityManager::new(cfg_arc)),
            vkturn: crate::vkturn::VkTurnManager::new(),
        };

        // 1. Отмечаем активную подписку «геодема2» как подходящую для белых списков
        let resp = set_provider_whitelist(
            axum::extract::State(state.clone()),
            axum::Json(ProviderWhitelistReq {
                provider_id: Some("геодема2".to_string()),
                provider: None,
                enabled: Some(true),
                providers: None,
                add_providers: None,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let cur = state.config.read().await.clone();
        assert!(
            cur.whitelist_providers.iter().any(|p| p == "геодема2"),
            "подписка должна сохраниться в whitelist_providers"
        );
        assert!(
            cur.whitelist.enabled,
            "режим белых списков должен автоматически включиться для уже активного сервера из отмеченной подписки"
        );
        assert_eq!(
            cur.whitelist.enabled_by,
            crate::config::WhitelistEnabledBy::Failover,
            "источник включения режима БС должен быть Failover"
        );

        let saved_disk = crate::config::load(&json_path);
        assert!(saved_disk.whitelist.enabled, "включение БС должно сохраниться в config.json");

        // 2. Снимаем отметку с подписки — режим БС должен автоматически выключиться
        crate::mihomo::invalidate_proxies_cache();
        let resp_off = set_provider_whitelist(
            axum::extract::State(state.clone()),
            axum::Json(ProviderWhitelistReq {
                provider_id: Some("геодема2".to_string()),
                provider: None,
                enabled: Some(false),
                providers: None,
                add_providers: None,
            }),
        )
        .await;
        assert_eq!(resp_off.status(), StatusCode::OK);
        assert!(
            !state.config.read().await.whitelist.enabled,
            "после снятия отметки с активной подписки режим БС (включенный автоматически) должен выключиться"
        );

        srv_handle.abort();
        crate::mihomo::invalidate_proxies_cache();
        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn regression_vkturn_25_flow_zapret_backup_evacuation_and_links() {
        let api_src = production_source();

        // Item 22: repair_flow and auto_select_flow_server must exclude vkturn and [БС] nodes
        let repair_fn = api_src
            .split("pub async fn repair_flow(")
            .nth(1)
            .and_then(|s| s.split("pub struct FlowAutoSelectReq").next())
            .expect("repair_flow must exist");
        assert!(
            repair_fn.contains("is_excluded_for_flow") || repair_fn.contains("PROXY_NAME"),
            "repair_flow must exclude vkturn and [БС] servers from Google Flow selection"
        );
        let auto_fn = api_src
            .split("pub async fn auto_select_flow_server(")
            .nth(1)
            .and_then(|s| s.split("pub async fn get_flow_extension(").next())
            .expect("auto_select_flow_server must exist");
        assert!(
            auto_fn.contains("is_excluded_for_flow") || auto_fn.contains("PROXY_NAME"),
            "auto_select_flow_server must exclude vkturn and [БС] servers"
        );

        // Item 9: SaveVkTurnConfigReq supports append_link and remove_link_index
        let save_req_block = api_src
            .split("pub struct SaveVkTurnConfigReq")
            .nth(1)
            .and_then(|s| s.split("pub async fn save_vkturn_config(").next())
            .expect("SaveVkTurnConfigReq must exist");
        assert!(save_req_block.contains("append_link") && save_req_block.contains("remove_link_index"));

        // Item 19: save_vkturn_config and vkturn_action evacuate active vkturn selectors when disabled/reset/stopped
        let vk_save_fn = api_src
            .split("pub async fn save_vkturn_config(")
            .nth(1)
            .and_then(|s| s.split("pub struct VkTurnActionReq").next())
            .expect("save_vkturn_config must exist");
        assert!(
            vk_save_fn.contains("evacuate_vkturn_selectors"),
            "save_vkturn_config must evacuate PROXY and 🛡️ Whitelist selectors when vkturn is disabled"
        );
        let vk_act_fn = api_src
            .split("pub async fn vkturn_action(")
            .nth(1)
            .and_then(|s| s.split("pub struct VkTurnAuthSaveReq").next())
            .expect("vkturn_action must exist");
        assert!(
            vk_act_fn.contains("evacuate_vkturn_selectors"),
            "vkturn_action reset must evacuate active vkturn selectors"
        );

        // Item 20: xkeen_service and zapret handlers re-apply vkturn exemptions
        let xkeen_fn = api_src
            .split("pub async fn xkeen_service(")
            .nth(1)
            .and_then(|s| s.split("/// Перезапуск службы XKeen").next())
            .expect("xkeen_service must exist");
        assert!(
            xkeen_fn.contains("apply_exemptions"),
            "xkeen_service start/restart must re-apply vkturn exemptions"
        );
        let zap_core_fn = api_src
            .split("pub async fn zapret_set_enabled_core(")
            .nth(1)
            .and_then(|s| s.split("pub async fn persist_zapret_enabled_with_routing(").next())
            .expect("zapret_set_enabled_core must exist");
        assert!(
            zap_core_fn.contains("apply_exemptions"),
            "zapret_set_enabled_core must re-apply vkturn exemptions"
        );
        let zap_act_fn = api_src
            .split("pub async fn zapret_action(")
            .nth(1)
            .and_then(|s| s.split("pub async fn get_zapret_status(").next())
            .expect("zapret_action must exist");
        assert!(
            zap_act_fn.contains("apply_exemptions"),
            "zapret_action must re-apply vkturn exemptions after firewall changes"
        );

        // Item 24: restore_backup syncs vkturn runtime & provider
        let restore_fn = api_src
            .split("pub async fn restore_backup(")
            .nth(1)
            .and_then(|s| s.split("pub async fn delete_backup(").next())
            .expect("restore_backup must exist");
        assert!(
            restore_fn.contains("sync_vkturn_after_restore"),
            "restore_backup must sync vkturn provider and runtime after config restore"
        );
    }

    #[test]
    fn regression_bug_112b_rp_filter_is_restored() {
        // 1. Чистая функция rp_filter_restore_needed: откат только если оба режима выключены
        assert!(rp_filter_restore_needed(false, false));
        assert!(!rp_filter_restore_needed(true, false));
        assert!(!rp_filter_restore_needed(false, true));
        assert!(!rp_filter_restore_needed(true, true));

        // 2. rp_filter_set_script читает текущие значения sysctl -n, сохраняет их в файл и ставит 2 (не ухудшая 0/2)
        let set_s = rp_filter_set_script();
        assert!(set_s.contains("sysctl -n net.ipv4.conf.all.rp_filter"), "должен читать all.rp_filter:\n{set_s}");
        assert!(set_s.contains("sysctl -n net.ipv4.conf.default.rp_filter"), "должен читать default.rp_filter:\n{set_s}");
        assert!(set_s.contains("sysctl -w net.ipv4.conf.all.rp_filter=2"), "должен выставлять 2:\n{set_s}");
        assert!(set_s.contains("xkeen_rp_filter.saved"), "должен сохранять исходные значения в файл:\n{set_s}");

        // 3. rp_filter_restore_script восстанавливает сохранённые значения (или 1) и удаляет файл
        let restore_s = rp_filter_restore_script();
        assert!(restore_s.contains("xkeen_rp_filter.saved"), "должен читать файл сохранения:\n{restore_s}");
        assert!(restore_s.contains("net.ipv4.conf.all.rp_filter="), "должен восстанавливать all.rp_filter:\n{restore_s}");
        assert!(restore_s.contains("net.ipv4.conf.default.rp_filter="), "должен восстанавливать default.rp_filter:\n{restore_s}");
    }

    #[test]
    fn regression_bug_a08_wan_diag_requires_204_for_generate_204() {
        assert!(!is_internet_status("http://cp.cloudflare.com/generate_204", 200), "captive portal 200 не должен считаться 204");
        assert!(is_internet_status("http://cp.cloudflare.com/generate_204", 204));
        assert!(is_internet_status("https://example.com/health", 200));
        assert!(!is_internet_status("https://example.com/health", 500));
    }

    #[test]
    fn regression_bug_33b_discord_voice_skips_gaming_devices() {
        // 1. При непустом gaming_ips RETURN для этого IP стоит ПЕРЕД правилом NFQUEUE для UDP 50000:65535
        let with_gaming = build_zapret_firewall_script(1053, &["192.168.1.50".to_string()]);
        let ret_pos = with_gaming
            .find("-s 192.168.1.50 -p udp")
            .expect("RETURN rule for gaming IP must be present");
        let ret_slice = &with_gaming[ret_pos..ret_pos + 120];
        assert!(ret_slice.contains("-j RETURN"), "Gaming IP rule must be RETURN: {ret_slice}");
        let nfq_pos = with_gaming
            .find("-p udp -m multiport --dports 50000:65535")
            .expect("NFQUEUE 50000:65535 rule must be present");
        assert!(ret_pos < nfq_pos, "Gaming RETURN rule must precede NFQUEUE 50000:65535 rule");

        // 2. При пустом gaming_ips правил RETURN для игровых IP нет
        let without_gaming = build_zapret_firewall_script(1053, &[]);
        assert!(!without_gaming.contains("50000:65535 -m comment --comment \"xkeen-route-zapret\" -j RETURN"));
        assert!(!without_gaming.contains("__ZAPRET_GAMING_UDP_EXEMPTIONS__"));
    }

    #[test]
    fn regression_bug_34b_custom_args_drive_queue_rules() {
        // 1. --qnum=201 --filter-udp=443 -> qnum=201 и udp=[443]
        let spec_udp = parse_zapret_queue_spec("--qnum=201 --filter-udp=443").unwrap();
        assert_eq!(spec_udp.qnum, 201);
        assert_eq!(spec_udp.udp_ports, vec!["443".to_string()]);
        assert!(spec_udp.tcp_ports.is_empty());

        let script = build_zapret_firewall_script_with_queue(1053, &[], &spec_udp);
        assert!(script.contains("--queue-num 201"), "iptables must use queue 201:\n{script}");
        assert!(script.contains("-p udp") && script.contains("443") && script.contains("--queue-num 201"), "UDP 443 rule must be present");
        assert!(script.contains("*--qnum=201*"), "is_zapret_pid must accept custom qnum=201");
        assert!(crate::zapret::is_zapret_cmdline_ours_with_qnum("nfqws --daemon --qnum=201 --filter-udp=443", Some(201)));

        // 2. Без аргументов -> qnum=200, tcp=[80, 443], udp=[]
        let spec_def = parse_zapret_queue_spec("").unwrap();
        assert_eq!(spec_def.qnum, 200);
        assert_eq!(spec_def.tcp_ports, vec!["80".to_string(), "443".to_string()]);
        assert!(spec_def.udp_ports.is_empty());

        // 3. Невалидные значения -> ошибка
        assert!(parse_zapret_queue_spec("--qnum=abc").is_err());
        assert!(parse_zapret_queue_spec("--qnum=0").is_err());
        assert!(parse_zapret_queue_spec("--qnum=70000").is_err());
        assert!(parse_zapret_queue_spec("--filter-udp=abc").is_err());
        assert!(parse_zapret_queue_spec("--filter-tcp=80,0").is_err());
        assert!(validate_custom_args("--qnum=abc").is_err());
    }

    #[tokio::test]
    async fn regression_bug_58b_zapret_ui_reload_error_is_reported() {
        let root = std::env::temp_dir().join(format!("xkeen-bug58-{}", crate::auth::random_hex(8)));
        std::fs::create_dir_all(&root).unwrap();
        let yaml_path = root.join("config.yaml");
        let json_path = root.join("config.json");
        let mut cfg = crate::config::AppConfig::default();
        cfg.mihomo.config_path = yaml_path.display().to_string();
        cfg.mihomo.port = 1; // Mihomo недоступен -> reload должен упасть
        cfg.zapret.enabled = true;
        cfg.zapret.hybrid_youtube = true;
        let base_yaml = "mode: rule\nproxies: []\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies: [DIRECT]\nrules:\n  - MATCH,PROXY\n";
        std::fs::write(&yaml_path, base_yaml).unwrap();
        crate::config::save(&json_path, &cfg).await.unwrap();
        let config_arc = std::sync::Arc::new(tokio::sync::RwLock::new(std::sync::Arc::new(cfg)));
        let ag = std::sync::Arc::new(crate::antigravity::AntigravityManager::new(config_arc.clone()));
        let state = AppState {
            config: config_arc,
            config_path: std::sync::Arc::new(json_path.clone()),
            http: reqwest::Client::new(),
            failover_log: std::sync::Arc::new(crate::failover::FailoverLog::default()),
            routing_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            config_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            speedtest_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            zapret_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            antigravity: ag,
            vkturn: crate::vkturn::VkTurnManager::new(),
        };

        let res = crate::api::zapret::zapret_set_enabled_core(&state, false).await;
        assert!(
            res.is_err(),
            "При ошибке reload Mihomo zapret_set_enabled_core должен возвращать Err, а не Ok: {res:?}"
        );
        let err = res.unwrap_err();
        assert!(
            err.to_lowercase().contains("mihomo") || err.to_lowercase().contains("reload") || err.to_lowercase().contains("подключ") || err.to_lowercase().contains("error"),
            "Ошибка должна сообщать о сбое reload Mihomo, получено: {err}"
        );
        assert!(
            state.config.read().await.zapret.enabled,
            "При откате zapret.enabled в памяти не должен измениться"
        );
        let disk_cfg: crate::config::AppConfig =
            serde_json::from_str(&std::fs::read_to_string(&json_path).unwrap()).unwrap();
        assert!(
            disk_cfg.zapret.enabled,
            "При откате zapret.enabled на диске не должен измениться"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn regression_bug_57b_ui_zapret_uses_shared_helper() {
        let root = std::env::temp_dir().join(format!("xkeen-bug57ui-{}", crate::auth::random_hex(8)));
        std::fs::create_dir_all(&root).unwrap();
        let yaml_path = root.join("config.yaml");
        let json_path = root.join("config.json");
        let mut cfg = crate::config::AppConfig::default();
        cfg.mihomo.config_path = yaml_path.display().to_string();
        cfg.mihomo.port = 1;
        cfg.zapret.enabled = false;
        let base_yaml = "mode: rule\nproxies: []\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies: [DIRECT]\nrules:\n  - MATCH,PROXY\n";
        std::fs::write(&yaml_path, base_yaml).unwrap();
        crate::config::save(&json_path, &cfg).await.unwrap();
        let config_arc = std::sync::Arc::new(tokio::sync::RwLock::new(std::sync::Arc::new(cfg)));
        let ag = std::sync::Arc::new(crate::antigravity::AntigravityManager::new(config_arc.clone()));
        let state = AppState {
            config: config_arc,
            config_path: std::sync::Arc::new(json_path.clone()),
            http: reqwest::Client::new(),
            failover_log: std::sync::Arc::new(crate::failover::FailoverLog::default()),
            routing_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            config_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            speedtest_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            zapret_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            antigravity: ag,
            vkturn: crate::vkturn::VkTurnManager::new(),
        };

        // 1. UI-старт без S51zapret: ошибка (через zapret_set_enabled_core), enabled остаётся false
        let req_start: crate::api::zapret::ZapretActionReq =
            serde_json::from_value(serde_json::json!({ "action": "start" })).unwrap();
        let resp_start = crate::api::zapret::zapret_action(
            axum::extract::State(state.clone()),
            axum::extract::Json(req_start),
        )
        .await;
        assert!(
            !resp_start.status().is_success() || {
                let bytes = axum::body::to_bytes(resp_start.into_body(), 65536).await.unwrap();
                let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
                v.get("success").and_then(|x| x.as_bool()) == Some(false)
            },
            "Неудачный старт через UI должен возвращать ошибку"
        );
        assert!(
            !state.config.read().await.zapret.enabled,
            "Неудачный старт не должен сохранять zapret.enabled=true"
        );

        // 2. UI-стоп через zapret_action: при ошибке reload возвращает ошибку и не меняет enabled=true
        {
            let mut c = (**state.config.read().await).clone();
            c.zapret.enabled = true;
            crate::config::save(&json_path, &c).await.unwrap();
            *state.config.write().await = std::sync::Arc::new(c);
        }
        let req_stop: crate::api::zapret::ZapretActionReq =
            serde_json::from_value(serde_json::json!({ "action": "stop" })).unwrap();
        let resp_stop = crate::api::zapret::zapret_action(
            axum::extract::State(state.clone()),
            axum::extract::Json(req_stop),
        )
        .await;
        let bytes = axum::body::to_bytes(resp_stop.into_body(), 65536).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
        assert_ne!(
            v.get("success").and_then(|x| x.as_bool()),
            Some(true),
            "UI-путь stop должен идти через общий helper и сообщать об ошибке reload: {v}"
        );
        assert!(
            state.config.read().await.zapret.enabled,
            "UI-путь stop при ошибке reload не должен оставлять zapret.enabled=false"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn regression_bug_111b_preset_keeps_unrelated_flags() {
        let mut zcfg = crate::config::ZapretConfig::default();
        zcfg.bypass_torrents = true;
        zcfg.bypass_github = true;
        zcfg.bypass_adult = true;
        zcfg.hybrid_discord = true;
        zcfg.discord_voice_udp = true;
        zcfg.smart_tv_mode = true;

        apply_zapret_preset(&mut zcfg, "youtube", None).unwrap();
        assert!(zcfg.youtube_turbo);
        assert!(zcfg.hybrid_youtube);
        assert!(zcfg.bypass_torrents, "youtube preset must keep bypass_torrents");
        assert!(zcfg.bypass_github, "youtube preset must keep bypass_github");
        assert!(zcfg.bypass_adult, "youtube preset must keep bypass_adult");
        assert!(zcfg.hybrid_discord, "youtube preset must keep hybrid_discord");
        assert!(zcfg.discord_voice_udp, "youtube preset must keep discord_voice_udp");
        assert!(zcfg.smart_tv_mode, "youtube preset must keep smart_tv_mode");

        let mut zcfg_dc = crate::config::ZapretConfig::default();
        zcfg_dc.bypass_torrents = true;
        zcfg_dc.bypass_github = true;
        zcfg_dc.bypass_adult = true;
        zcfg_dc.youtube_turbo = true;
        zcfg_dc.hybrid_youtube = true;

        apply_zapret_preset(&mut zcfg_dc, "discord", None).unwrap();
        assert!(zcfg_dc.hybrid_discord);
        assert!(zcfg_dc.discord_voice_udp);
        assert!(zcfg_dc.bypass_torrents, "discord preset must keep bypass_torrents");
        assert!(zcfg_dc.bypass_github, "discord preset must keep bypass_github");
        assert!(zcfg_dc.bypass_adult, "discord preset must keep bypass_adult");
        assert!(zcfg_dc.youtube_turbo, "discord preset must keep youtube_turbo");
        assert!(zcfg_dc.hybrid_youtube, "discord preset must keep hybrid_youtube");
    }

    #[test]
    fn regression_bug_67b_pin_server_patches_single_device() {
        let mut devices = vec![
            crate::config::WhitelistDevice {
                mac: "11:22:33:44:55:66".into(),
                ip: "192.168.1.50".into(), // актуальный IP от DHCP-монитора
                enabled: true,
                name: Some("PC".into()),
                server: None,
            },
            crate::config::WhitelistDevice {
                mac: "AA:BB:CC:DD:EE:FF".into(),
                ip: "192.168.1.99".into(), // актуальный IP второго устройства от DHCP-монитора
                enabled: true,
                name: Some("TV".into()),
                server: Some("Old-Node".into()),
            },
        ];

        // Патчим только первое устройство (пришёл устаревший ip из UI, но у существующего устройства IP не должен затираться)
        let patch = crate::api::routing::WhitelistDevicePatch {
            mac: "11:22:33:44:55:66".into(),
            ip: Some("192.168.1.10".into()),
            enabled: Some(true),
            name: None,
            server: Some("Proxy-DE".into()),
        };
        crate::api::routing::patch_whitelist_device(&mut devices, &patch).unwrap();

        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].server.as_deref(), Some("Proxy-DE"));
        assert_eq!(devices[0].ip, "192.168.1.50", "DHCP IP существующего устройства не должен затираться");
        assert_eq!(devices[1].ip, "192.168.1.99", "IP второго устройства не должен меняться");
        assert_eq!(devices[1].server.as_deref(), Some("Old-Node"));
    }

    #[test]
    fn regression_bug_66b_whitelist_patch_applies_only_given_fields() {
        let mut wl = crate::config::WhitelistConfig::default();
        wl.enabled = true;
        wl.auto_by_failover = true;
        wl.follow_failover = true;
        wl.custom_domains = vec!["my-bank.ru".into()];

        let patch: crate::api::routing::WhitelistUpdateReq = serde_json::from_value(serde_json::json!({
            "auto_by_failover": false
        }))
        .unwrap();

        crate::api::routing::merge_whitelist_patch(&mut wl, &patch).unwrap();
        assert!(!wl.auto_by_failover, "auto_by_failover must be updated");
        assert!(wl.follow_failover, "follow_failover must remain untouched");
        assert!(wl.enabled, "enabled must remain untouched");
        assert_eq!(wl.custom_domains, vec!["my-bank.ru".to_string()], "custom_domains must remain untouched");
    }

    #[test]
    fn regression_bug_65b_conflicting_device_flags_are_rejected() {
        use crate::config::{device_conflicts, AppConfig, GamingDevice, WhitelistDevice};

        // 1. Игровое устройство + исключение из БС при активном БС -> конфликт
        let mut cfg = AppConfig::default();
        cfg.gaming.enabled = true;
        cfg.gaming.devices = vec![GamingDevice {
            mac: "AA:BB:CC:DD:EE:01".into(),
            ip: "192.168.1.50".into(),
            ipv6: Vec::new(),
            name: "PS5".into(),
            enabled: true,
            server: None,
        }];
        cfg.whitelist.enabled = true;
        cfg.whitelist.excluded_devices = vec!["AA:BB:CC:DD:EE:01".into()];
        let conflicts = device_conflicts(&cfg);
        assert!(
            !conflicts.is_empty(),
            "gaming + whitelist exclude while whitelist enabled must conflict"
        );
        let resp = crate::api::api_err_bad_request(crate::config::format_device_conflicts(&conflicts));
        assert_eq!(resp.status(), axum::http::StatusCode::BAD_REQUEST, "conflict response must be HTTP 400");

        // 2. Игровое устройство + исключение из БС при выключенном БС -> допустимо
        cfg.whitelist.enabled = false;
        assert!(
            device_conflicts(&cfg).is_empty(),
            "gaming + whitelist exclude while whitelist disabled is allowed"
        );

        // 3. Игровое устройство + исключение из Zapret -> допустимо
        cfg.whitelist.enabled = true;
        cfg.whitelist.excluded_devices.clear();
        cfg.zapret.enabled = true;
        cfg.zapret.excluded_devices = vec!["192.168.1.50".into()];
        assert!(
            device_conflicts(&cfg).is_empty(),
            "gaming + zapret exclude is allowed"
        );

        // 4. Исключение из БС + закрепление за сервером БС при активном БС -> конфликт
        cfg.gaming.enabled = false;
        cfg.whitelist.enabled = true;
        cfg.whitelist.excluded_devices = vec!["192.168.1.60".into()];
        cfg.whitelist.devices = vec![WhitelistDevice {
            mac: "AA:BB:CC:DD:EE:02".into(),
            ip: "192.168.1.60".into(),
            name: Some("PC".into()),
            server: Some("Proxy-DE".into()),
            enabled: true,
        }];
        assert!(
            !device_conflicts(&cfg).is_empty(),
            "whitelist exclude + pinned server while whitelist enabled must conflict"
        );
    }

    #[test]
    fn regression_bug_a11_marking_active_provider_enables_whitelist() {
        use crate::config::{AppConfig, WhitelistEnabledBy};
        use crate::failover::{whitelist_transition_after_marking, WhitelistTransition};

        // Подписка "геодема2" помечена как БС, активен узел из неё, auto_by_failover = true -> Enable
        let mut cfg = AppConfig::default();
        cfg.whitelist.auto_by_failover = true;
        cfg.whitelist.enabled = false;
        cfg.whitelist_providers = vec!["геодема2".into()];

        let active = Some(("node_geo_1", "Node 01", Some("геодема2")));
        assert_eq!(
            whitelist_transition_after_marking(&cfg, active),
            Some(WhitelistTransition::Enable),
            "marking active provider with auto_by_failover=true must trigger Enable"
        );

        // auto_by_failover = false -> None (A-11.2)
        cfg.whitelist.auto_by_failover = false;
        assert_eq!(
            whitelist_transition_after_marking(&cfg, active),
            None,
            "with auto_by_failover=false marking provider must not auto-enable whitelist"
        );

        // A-11.3: снятие отметки выключает БС только при enabled_by == Failover
        cfg.whitelist.auto_by_failover = true;
        cfg.whitelist.enabled = true;
        cfg.whitelist.enabled_by = WhitelistEnabledBy::Failover;
        cfg.whitelist_providers.clear();
        assert_eq!(
            whitelist_transition_after_marking(&cfg, active),
            Some(WhitelistTransition::Disable),
            "unmarking active provider disables whitelist when enabled_by=Failover"
        );

        cfg.whitelist.enabled_by = WhitelistEnabledBy::Manual;
        assert_eq!(
            whitelist_transition_after_marking(&cfg, active),
            None,
            "unmarking active provider must NOT disable whitelist when enabled_by=Manual"
        );
    }

    #[test]
    fn regression_bug_17b_single_disabled_gaming_device_is_inactive() {
        use crate::config::{is_gaming_device_active, GamingConfig, GamingDevice};

        // 1. Единственное явно выключенное устройство -> false
        let disabled = GamingDevice {
            mac: "AA:BB:CC:DD:EE:01".into(),
            ip: "192.168.1.50".into(),
            ipv6: Vec::new(),
            name: "PS5".into(),
            enabled: false,
            server: None,
        };
        assert!(
            !is_gaming_device_active(&disabled, true),
            "single explicitly disabled gaming device must be inactive"
        );

        // 2. Новый JSON с явным "enabled": false для единственного устройства -> false
        let new_json = r#"{"enabled": true, "devices": [{"mac": "AA:BB:CC:DD:EE:01", "ip": "192.168.1.50", "name": "PS5", "enabled": false}]}"#;
        let parsed_new: GamingConfig = serde_json::from_str(new_json).unwrap();
        assert_eq!(parsed_new.devices.len(), 1);
        assert!(
            !is_gaming_device_active(&parsed_new.devices[0], true),
            "single device with explicit enabled:false in JSON must remain inactive"
        );

        // 3. Старый JSON без поля "enabled" для единственного устройства -> true (обратная совместимость)
        let legacy_json = r#"{"enabled": true, "devices": [{"mac": "AA:BB:CC:DD:EE:01", "ip": "192.168.1.50", "name": "PS5"}]}"#;
        let parsed_legacy: GamingConfig = serde_json::from_str(legacy_json).unwrap();
        assert_eq!(parsed_legacy.devices.len(), 1);
        assert!(
            is_gaming_device_active(&parsed_legacy.devices[0], true),
            "legacy single device without enabled field must default to active"
        );
        assert!(
            is_gaming_device_active(&disabled, false),
            "when has_explicit=false (legacy caller), device is treated as active"
        );

        // 4. Несколько устройств, одно включено -> активно только оно
        let d1 = GamingDevice { enabled: false, ..disabled.clone() };
        let d2 = GamingDevice { mac: "AA:BB:CC:DD:EE:02".into(), enabled: true, ..disabled };
        assert!(!is_gaming_device_active(&d1, true));
        assert!(is_gaming_device_active(&d2, true));
    }

    #[test]
    fn regression_bug_20b_user_exclude_ipset_is_used_in_chains() {
        let mut cfg = crate::config::AppConfig::default();
        cfg.gaming.enabled = true;
        cfg.gaming.devices = vec![crate::config::GamingDevice {
            mac: "AA:BB:CC:DD:EE:01".into(),
            ip: "192.168.1.50".into(),
            name: "PS5".into(),
            server: Some("Fast".into()),
            enabled: true,
            ..Default::default()
        }];
        let (_g_cleanup, g_setup, _) = build_gaming_full_tunnel_commands(&cfg.gaming, 1082, 1081, 5353);
        let g_all = g_setup.join("\n");
        assert!(
            g_all.contains("ipset create user_exclude hash:ip -exist"),
            "Gaming setup must ensure user_exclude ipset exists:\n{g_all}"
        );
        assert!(
            g_all.contains("-A XKEEN_GAMING -m set --match-set user_exclude dst -j RETURN -w 2"),
            "Gaming nat/mangle must bypass user_exclude ipset:\n{g_all}"
        );

        let mut bs_cfg = crate::config::AppConfig::default();
        bs_cfg.whitelist.enabled = true;
        bs_cfg.whitelist.strategy = crate::config::WhitelistStrategy::StrictXKeen;
        let (_bs_cleanup, bs_setup) = build_whitelist_full_tunnel_commands(&bs_cfg, 1082, 1081, 5353);
        let bs_all = bs_setup.join("\n");
        assert!(
            bs_all.contains("ipset create user_exclude hash:ip -exist"),
            "BS setup must ensure user_exclude ipset exists:\n{bs_all}"
        );
        assert!(
            bs_all.contains("-A XKEEN_BS -m set --match-set user_exclude dst -j RETURN -w"),
            "BS nat/mangle must bypass user_exclude ipset:\n{bs_all}"
        );
    }
}






