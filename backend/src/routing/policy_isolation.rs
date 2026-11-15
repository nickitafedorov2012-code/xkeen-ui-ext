//! Изоляция устройств с политикой по умолчанию (`default`) в KeeneticOS.
//!
//! Если на роутере присутствует политика XKeen (`Policy0` / `xkeen`), устройства,
//! находящиеся в политике по умолчанию (`default`), не должны перехватываться
//! правилами TPROXY (`iptables` / `ipset` `xkeen_deny_mac`, `user_exclude`) и
//! общим LAN catch-all правилом `StrictXKeen` в `mihomo` (`SRC-IP-CIDR,192.168.0.0/16,...`).

use std::collections::BTreeSet;
use std::net::Ipv4Addr;
use std::sync::{OnceLock, RwLock};

use crate::config::AppConfig;
use crate::rci::{Device, Policy};

static DEFAULT_POLICY_IPS: OnceLock<RwLock<Vec<String>>> = OnceLock::new();

fn cache_lock() -> &'static RwLock<Vec<String>> {
    DEFAULT_POLICY_IPS.get_or_init(|| RwLock::new(Vec::new()))
}

/// Получить текущий кэшированный список IPv4-адресов устройств с политикой `default`.
pub fn get_cached_default_policy_ips() -> Vec<String> {
    cache_lock()
        .read()
        .map(|g| g.clone())
        .unwrap_or_default()
}

/// Обновить кэш IPv4-адресов устройств с политикой `default`.
/// Возвращает `true`, если нормализованный список изменился.
pub fn set_cached_default_policy_ips(mut ips: Vec<String>) -> bool {
    ips.sort();
    ips.dedup();
    if let Ok(mut guard) = cache_lock().write() {
        if *guard != ips {
            *guard = ips;
            return true;
        }
    }
    false
}

/// Проверяет, настроена ли на роутере политика XKeen (`Policy0` или не-default политика с `xkeen` в названии).
/// Если политика отсутствует (установка без разделения политик), изоляция `default` не применяется.
pub fn has_xkeen_policy(policies: &[Policy]) -> bool {
    policies.iter().any(|p| {
        p.id == "Policy0"
            || (!p.is_default
                && p.id != "block"
                && p.name.to_lowercase().contains("xkeen"))
    })
}

fn is_valid_mac(mac: &str) -> bool {
    let parts: Vec<&str> = mac.split(':').collect();
    if parts.len() != 6 {
        return false;
    }
    if mac == "00:00:00:00:00:00" || mac == "ff:ff:ff:ff:ff:ff" {
        return false;
    }
    parts
        .iter()
        .all(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_hexdigit()))
}

fn parse_routable_lan_ipv4(ip: &str) -> Option<String> {
    let trimmed = ip.trim();
    let addr: Ipv4Addr = trimmed.parse().ok()?;
    if addr.is_unspecified() || addr.is_loopback() || addr.is_multicast() || addr.is_broadcast() {
        return None;
    }
    Some(addr.to_string())
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PolicyIsolationPlan {
    pub default_macs: Vec<String>,
    pub non_default_macs: Vec<String>,
    pub default_ips: Vec<String>,
    pub non_default_ips: Vec<String>,
}

/// Формирует план изоляции для устройств с политикой `default` в KeeneticOS.
/// Возвращает `None`, если на роутере нет политики XKeen (`Policy0`).
pub fn build_policy_isolation_plan(
    policies: &[Policy],
    devices: &[Device],
) -> Option<PolicyIsolationPlan> {
    if !has_xkeen_policy(policies) {
        return None;
    }

    let mut default_macs = BTreeSet::new();
    let mut non_default_macs = BTreeSet::new();
    let mut default_ips = BTreeSet::new();
    let mut non_default_ips = BTreeSet::new();

    for d in devices {
        let pol = d.policy.trim();
        let is_default = pol.is_empty() || pol == "default";
        let mac = d.mac.trim().to_lowercase();
        let ip = parse_routable_lan_ipv4(&d.ip);

        if is_default {
            if is_valid_mac(&mac) {
                default_macs.insert(mac);
            }
            if let Some(ip_str) = ip {
                default_ips.insert(ip_str);
            }
        } else {
            if is_valid_mac(&mac) {
                non_default_macs.insert(mac);
            }
            if let Some(ip_str) = ip {
                non_default_ips.insert(ip_str);
            }
        }
    }

    // При коллизии (например, дублирующая запись в RCI) приоритет у не-default политики.
    for m in &non_default_macs {
        default_macs.remove(m);
    }
    for ip in &non_default_ips {
        default_ips.remove(ip);
    }

    Some(PolicyIsolationPlan {
        default_macs: default_macs.into_iter().collect(),
        non_default_macs: non_default_macs.into_iter().collect(),
        default_ips: default_ips.into_iter().collect(),
        non_default_ips: non_default_ips.into_iter().collect(),
    })
}

/// Генерирует идемпотентные команды `ipset` для синхронизации `xkeen_deny_mac` и `user_exclude`.
pub fn build_ipset_sync_commands(
    plan: &PolicyIsolationPlan,
    existing_sets: &[&str],
    excluded_cfg_ips: &[String],
) -> Vec<(Vec<String>, bool)> {
    // Кортеж: (аргументы ipset, ignore_error_on_exec)
    let mut cmds = Vec::new();
    let has_deny_mac = existing_sets.iter().any(|s| s.trim() == "xkeen_deny_mac");
    let has_user_exclude = existing_sets.iter().any(|s| s.trim() == "user_exclude");

    if has_deny_mac {
        for mac in &plan.default_macs {
            cmds.push((
                vec![
                    "add".to_string(),
                    "xkeen_deny_mac".to_string(),
                    mac.clone(),
                    "-exist".to_string(),
                ],
                false,
            ));
        }
        for mac in &plan.non_default_macs {
            cmds.push((
                vec![
                    "del".to_string(),
                    "xkeen_deny_mac".to_string(),
                    mac.clone(),
                ],
                true,
            ));
        }
    }

    if has_user_exclude {
        for ip in &plan.default_ips {
            cmds.push((
                vec![
                    "add".to_string(),
                    "user_exclude".to_string(),
                    ip.clone(),
                    "-exist".to_string(),
                ],
                false,
            ));
        }
        for ip in &plan.non_default_ips {
            if excluded_cfg_ips.iter().any(|ex| ex.trim() == ip) {
                continue;
            }
            cmds.push((
                vec![
                    "del".to_string(),
                    "user_exclude".to_string(),
                    ip.clone(),
                ],
                true,
            ));
        }
    }

    cmds
}

#[cfg(target_os = "linux")]
async fn sync_ipsets_linux(plan: &PolicyIsolationPlan, excluded_cfg_ips: &[String]) {
    let Ok(out) = tokio::process::Command::new("ipset")
        .args(["list", "-n"])
        .output()
        .await
    else {
        return;
    };
    if !out.status.success() {
        return;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let existing_sets: Vec<&str> = stdout.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let cmds = build_ipset_sync_commands(plan, &existing_sets, excluded_cfg_ips);
    for (args, ignore_err) in cmds {
        let res = tokio::process::Command::new("ipset")
            .args(&args)
            .output()
            .await;
        if !ignore_err {
            if let Ok(o) = res {
                if !o.status.success() {
                    let err = String::from_utf8_lossy(&o.stderr);
                    crate::log_w!("[POLICY-ISOLATION] ipset {:?} ошибка: {}", args, err.trim());
                }
            }
        }
    }
}

#[cfg(not(target_os = "linux"))]
async fn sync_ipsets_linux(_plan: &PolicyIsolationPlan, _excluded_cfg_ips: &[String]) {}

/// Синхронизировать изоляцию `default`-устройств с готовыми списками политик и устройств.
pub async fn sync_default_policy_isolation_with_data(
    state: &crate::AppState,
    cfg: &AppConfig,
    policies: &[Policy],
    devices: &[Device],
) {
    let Some(plan) = build_policy_isolation_plan(policies, devices) else {
        let changed = set_cached_default_policy_ips(Vec::new());
        if changed
            && cfg.whitelist.enabled
            && cfg.whitelist.strategy == crate::config::WhitelistStrategy::StrictXKeen
        {
            rebuild_and_reload_mihomo(state, cfg).await;
        }
        return;
    };

    let excluded_cfg_ips = crate::routing::resolve_excluded_whitelist_ips(cfg);
    sync_ipsets_linux(&plan, &excluded_cfg_ips).await;

    let ips_changed = set_cached_default_policy_ips(plan.default_ips);
    if ips_changed
        && cfg.whitelist.enabled
        && cfg.whitelist.strategy == crate::config::WhitelistStrategy::StrictXKeen
    {
        rebuild_and_reload_mihomo(state, cfg).await;
    }
}

/// Получить политики/устройства из RCI и синхронизировать изоляцию `default`-устройств.
pub async fn sync_default_policy_isolation(state: &crate::AppState) {
    let cfg = state.config.read().await.clone();
    let Ok(policies) = crate::rci::get_policies(&state.http, &cfg).await else {
        return;
    };
    let Ok(devices) = crate::rci::get_devices(&state.http, &cfg, &policies, "").await else {
        return;
    };
    sync_default_policy_isolation_with_data(state, &cfg, &policies, &devices).await;
}

async fn rebuild_and_reload_mihomo(state: &crate::AppState, cfg: &AppConfig) {
    let _guard = state.routing_lock.lock().await;
    let yaml_path = std::path::Path::new(&cfg.mihomo.config_path);
    let Ok(raw_yaml) = tokio::fs::read_to_string(yaml_path).await else {
        return;
    };
    let Ok((new_yaml, _)) = crate::routing::apply_routing(&raw_yaml, cfg) else {
        return;
    };
    if new_yaml == raw_yaml {
        return;
    }
    if crate::transaction::validate_yaml_syntax(&new_yaml).is_err() {
        crate::log_w!("[POLICY-ISOLATION] Сгенерированный YAML не прошёл валидацию, пропуск");
        return;
    }
    if let Err(e) = tokio::fs::write(yaml_path, &new_yaml).await {
        crate::log_w!("[POLICY-ISOLATION] Ошибка записи {}: {e}", yaml_path.display());
        return;
    }
    if let Err(e) = crate::mihomo::reload_config(&state.http, cfg).await {
        crate::log_w!("[POLICY-ISOLATION] Ошибка reload Mihomo: {e}");
    } else {
        crate::log_i!("[POLICY-ISOLATION] Правила изоляции default-политики обновлены в Mihomo");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_policy(id: &str, name: &str, is_default: bool) -> Policy {
        Policy {
            id: id.into(),
            name: name.into(),
            is_default,
        }
    }

    fn mk_device(mac: &str, ip: &str, policy: &str) -> Device {
        Device {
            mac: mac.into(),
            name: "Dev".into(),
            ip: ip.into(),
            policy: policy.into(),
            policy_name: String::new(),
            online: true,
            interface: "Home".into(),
            is_current_device: false,
            rxbytes: 0,
            txbytes: 0,
            speed_limit_kbps: 0,
            ipv6: vec![],
        }
    }

    #[test]
    fn test_no_xkeen_policy_skips_isolation() {
        let policies = vec![mk_policy("default", "🌐 Политика по умолчанию", true)];
        let devs = vec![mk_device("de:0d:ab:54:2f:6b", "192.168.2.52", "default")];
        assert!(build_policy_isolation_plan(&policies, &devs).is_none());
    }

    #[test]
    fn test_default_policy_device_isolated_while_policy0_preserved() {
        let policies = vec![
            mk_policy("default", "🌐 Политика по умолчанию", true),
            mk_policy("Policy0", "🛡️ XKeen", false),
        ];
        let devs = vec![
            mk_device("DE:0D:AB:54:2F:6B", "192.168.2.52", "default"),
            mk_device("D2:0D:AB:54:2F:6B", "0.0.0.0", "Policy0"),
            mk_device("AA:BB:CC:DD:EE:01", "192.168.2.10", "Policy0"),
        ];

        let plan = build_policy_isolation_plan(&policies, &devs).expect("plan must exist");
        assert_eq!(plan.default_macs, vec!["de:0d:ab:54:2f:6b"]);
        assert_eq!(plan.default_ips, vec!["192.168.2.52"]);
        assert_eq!(
            plan.non_default_macs,
            vec!["aa:bb:cc:dd:ee:01", "d2:0d:ab:54:2f:6b"]
        );
        assert_eq!(plan.non_default_ips, vec!["192.168.2.10"]);
    }

    #[test]
    fn test_ipset_commands_preserve_manual_exclusions() {
        let plan = PolicyIsolationPlan {
            default_macs: vec!["de:0d:ab:54:2f:6b".into()],
            non_default_macs: vec!["aa:bb:cc:dd:ee:01".into()],
            default_ips: vec!["192.168.2.52".into()],
            non_default_ips: vec!["192.168.2.10".into(), "192.168.2.99".into()],
        };
        let cmds = build_ipset_sync_commands(
            &plan,
            &["xkeen_deny_mac", "user_exclude"],
            &["192.168.2.99".into()],
        );
        assert!(cmds.iter().any(|(a, _)| a == &["add", "xkeen_deny_mac", "de:0d:ab:54:2f:6b", "-exist"]));
        assert!(cmds.iter().any(|(a, _)| a == &["del", "xkeen_deny_mac", "aa:bb:cc:dd:ee:01"]));
        assert!(cmds.iter().any(|(a, _)| a == &["add", "user_exclude", "192.168.2.52", "-exist"]));
        assert!(cmds.iter().any(|(a, _)| a == &["del", "user_exclude", "192.168.2.10"]));
        assert!(
            !cmds.iter().any(|(a, _)| a.iter().any(|x| x == "192.168.2.99")),
            "manual excluded IP must NOT be removed from user_exclude"
        );
    }

    #[test]
    fn test_strict_xkeen_places_default_policy_ip_direct_above_lan_catchall() {
        let mut cfg = AppConfig::default();
        cfg.whitelist.enabled = true;
        cfg.whitelist.strategy = crate::config::WhitelistStrategy::StrictXKeen;
        cfg.whitelist.enabled_by = crate::config::WhitelistEnabledBy::Manual;

        let base_yaml = "port: 7890\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies:\n      - DIRECT\nrules:\n  - MATCH,PROXY\n";
        let with_default = crate::routing::apply_whitelist_rules_with_default_ips(
            base_yaml,
            &cfg,
            Some("PROXY"),
            &["192.168.2.52".to_string()],
        );
        let direct_pos = with_default
            .find("- SRC-IP-CIDR,192.168.2.52/32,DIRECT")
            .expect("default-policy IP must get DIRECT rule");
        let catchall_pos = with_default
            .find("- SRC-IP-CIDR,192.168.0.0/16,🛡️ Whitelist")
            .expect("StrictXKeen LAN catch-all must exist");
        assert!(
            direct_pos < catchall_pos,
            "default-policy DIRECT rule must appear above LAN catch-all"
        );

        let after_move_to_policy0 = crate::routing::apply_whitelist_rules_with_default_ips(
            &with_default,
            &cfg,
            Some("PROXY"),
            &[],
        );
        assert!(
            !after_move_to_policy0.contains("192.168.2.52/32,DIRECT"),
            "moving device out of default must remove its DIRECT isolation rule"
        );
    }
}
