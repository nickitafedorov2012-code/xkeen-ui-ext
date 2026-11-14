use super::*;

pub fn get_all_whitelist_domains(whitelist_cfg: &crate::config::WhitelistConfig) -> Vec<String> {
    let mut out: Vec<String> = DOMESTIC_WHITELIST_DOMAINS.iter().map(|&s| s.to_string()).collect();
    for d in &whitelist_cfg.custom_domains {
        let clean = crate::config::normalize_domain(d);
        if !clean.is_empty() && crate::config::is_valid_domain(&clean) && !out.contains(&clean) {
            out.push(clean);
        }
    }
    out
}

/// Удаление блока управляемого отклонения QUIC (UDP/443).
pub fn remove_quic_reject_rules(yaml: &str) -> String {
    let mut out = Vec::new();
    let mut skipping = false;
    for line in yaml.lines() {
        let t = line.trim();
        if t == QUIC_REJECT_BEGIN {
            skipping = true;
            continue;
        }
        if t == QUIC_REJECT_END {
            skipping = false;
            continue;
        }
        if skipping {
            continue;
        }
        out.push(line);
    }
    out.join("\n")
}

/// QUIC-ORD-01: Генерация и вставка блока отклонения QUIC строго ДО # --- AUTO-WHITELIST-BEGIN ---
/// и строго ДО всех правил SRC-IP-CIDR (как в обычном режиме, так и в StrictXKeen).
pub fn apply_quic_reject_rules(yaml: &str) -> String {
    let cleaned = remove_quic_reject_rules(yaml);
    let lines: Vec<&str> = cleaned.lines().collect();
    let rules_pos = match lines.iter().position(|l| l.trim_end() == "rules:") {
        Some(pos) => pos,
        None => return cleaned,
    };

    let mut target_idx = None;
    let mut in_other_marker = false;

    for (i, line) in lines.iter().enumerate().skip(rules_pos + 1) {
        let t = line.trim();
        if t.starts_with("# --- AUTO-") && t.ends_with("-BEGIN ---") {
            if t == GAMING_BEGIN || t == WHITELIST_BEGIN || t == RULES_BEGIN {
                target_idx = Some(i);
                break;
            }
            in_other_marker = true;
            continue;
        }
        if t.starts_with("# --- AUTO-") && t.ends_with("-END ---") {
            in_other_marker = false;
            continue;
        }
        if !in_other_marker && t.starts_with("- SRC-IP-CIDR,") {
            target_idx = Some(i);
            break;
        }
    }

    let insert_idx = match target_idx {
        Some(idx) => idx,
        None => {
            if let Some(fallback_idx) = lines[rules_pos + 1..].iter().position(|l| {
                let t = l.trim();
                t.starts_with("- GEOIP,") || t.starts_with("- MATCH,")
            }) {
                rules_pos + 1 + fallback_idx
            } else {
                rules_pos + 1
            }
        }
    };

    let quic_block = [
        QUIC_REJECT_BEGIN,
        QUIC_REJECT_RULE,
        QUIC_REJECT_END,
    ];

    let mut out = Vec::with_capacity(lines.len() + quic_block.len());
    for (i, line) in lines.iter().enumerate() {
        if i == insert_idx {
            for q in &quic_block {
                out.push(q.to_string());
            }
        }
        out.push(line.to_string());
    }
    if insert_idx >= lines.len() {
        for q in &quic_block {
            out.push(q.to_string());
        }
    }

    out.join("\n")
}

/// DNS-HE-01: Гарантирует наличие `tcp-concurrent: true` в корневой секции config.yaml
/// для активации RFC 8305 Happy Eyeballs параллельного дозвона в ядре Mihomo.
pub fn ensure_tcp_concurrent(yaml: &str) -> String {
    let eol = if yaml.contains("\r\n") { "\r\n" } else { "\n" };
    let had_trailing_eol = yaml.ends_with('\n');
    let mut lines: Vec<String> = yaml.lines().map(String::from).collect();
    let mut found = false;

    for line in &mut lines {
        let trimmed = line.trim();
        if trimmed == "tcp-concurrent: false" {
            let indent = line.len() - line.trim_start().len();
            let prefix = " ".repeat(indent);
            *line = format!("{prefix}tcp-concurrent: true");
            found = true;
            break;
        } else if trimmed == "tcp-concurrent: true" {
            found = true;
            break;
        }
    }

    if !found {
        let insert_idx = lines
            .iter()
            .position(|l| l.trim() == "unified-delay: true" || l.trim() == "unified-delay: false")
            .map(|i| i + 1)
            .or_else(|| {
                lines.iter().position(|l| {
                    let t = l.trim();
                    t == "proxies:" || t == "proxy-groups:" || t == "proxy-providers:" || t == "rules:"
                })
            })
            .unwrap_or(0);

        lines.insert(insert_idx, "tcp-concurrent: true".to_string());
    }

    let mut joined = lines.join(eol);
    if had_trailing_eol && !joined.ends_with(eol) {
        joined.push_str(eol);
    }
    joined
}

/// Удаление блока правил и селектор-группы режима белых списков из YAML.
pub fn remove_whitelist_rules(yaml: &str) -> String {
    let mut out = Vec::new();
    let mut skip_rules = false;
    let mut skip_group = false;
    for line in yaml.lines() {
        let t = line.trim();
        if t == WHITELIST_BEGIN {
            skip_rules = true;
            continue;
        }
        if t == WHITELIST_END {
            skip_rules = false;
            continue;
        }
        if t == WHITELIST_GROUP_BEGIN {
            skip_group = true;
            continue;
        }
        if t == WHITELIST_GROUP_END {
            skip_group = false;
            continue;
        }
        if !skip_rules && !skip_group {
            out.push(line);
        }
    }
    out.join("\n")
}

/// Удаление DNS-правил nameserver-policy режима белых списков из YAML.
pub fn remove_whitelist_dns(yaml: &str) -> String {
    if !yaml.contains(WHITELIST_DNS_BEGIN) {
        return yaml.to_string();
    }
    let mut out = Vec::new();
    let mut skipping = false;
    for line in yaml.lines() {
        let t = line.trim();
        if t == WHITELIST_DNS_BEGIN {
            skipping = true;
            continue;
        }
        if t == WHITELIST_DNS_END {
            skipping = false;
            continue;
        }
        if !skipping {
            out.push(line);
        }
    }
    out.join("\n")
}

/// Применение правил режима белых списков:
/// 1. Вставка селектор-группы '🛡️ Whitelist' в proxy-groups:
/// 2. Вставка блока # --- AUTO-WHITELIST-BEGIN --- в rules:
///    - В StrictXKeen весь внешний трафик LAN по умолчанию -> 🛡️ Whitelist
///    - DIRECT остаётся только для явных пользовательских исключений и исключённых устройств
/// Поиск IP адреса по MAC в содержимом ARP таблицы (/proc/net/arp)
pub fn find_ip_for_mac_in_arp(mac: &str, arp_data: &str) -> Option<String> {
    let clean_mac = mac.trim().to_ascii_lowercase();
    if clean_mac.is_empty() {
        return None;
    }
    for line in arp_data.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 4 {
            let ip = parts[0];
            let hw_addr = parts[3].to_ascii_lowercase();
            if hw_addr == clean_mac && crate::config::is_valid_ip_or_cidr(ip) {
                return Some(ip.to_string());
            }
        }
    }
    None
}

/// Разрешение IP-адресов исключенных из белых списков устройств (по IP или MAC).
pub fn resolve_excluded_whitelist_ips(cfg: &crate::config::AppConfig) -> Vec<String> {
    resolve_excluded_whitelist_ips_internal(cfg, None)
}

pub fn resolve_excluded_whitelist_ips_internal(cfg: &crate::config::AppConfig, arp_content: Option<&str>) -> Vec<String> {
    let mut ips = Vec::new();

    // Читаем системную ARP-таблицу если содержимое не передано явно
    let system_arp = if arp_content.is_none() {
        std::fs::read_to_string("/proc/net/arp").ok()
    } else {
        None
    };
    let effective_arp = arp_content.or(system_arp.as_deref());

    // Системное исключение устройств с «Политикой по умолчанию» или выключенным БС (enabled: false):
    // Устройства, у которых в белом списке явно стоит enabled == false, обязаны идти напрямую.
    for dev in &cfg.whitelist.devices {
        if !dev.enabled {
            let mut resolved_ip = dev.ip.trim().to_string();
            if !dev.mac.trim().is_empty() {
                if let Some(arp_data) = effective_arp {
                    if let Some(ip) = find_ip_for_mac_in_arp(&dev.mac, arp_data) {
                        resolved_ip = ip;
                    }
                }
            }
            if !resolved_ip.is_empty() && crate::config::is_valid_ip_or_cidr(&resolved_ip) {
                ips.push(resolved_ip);
            }
        }
    }

    for item in &cfg.whitelist.excluded_devices {
        let clean = item.trim();
        if clean.is_empty() {
            continue;
        }
        if crate::config::is_valid_ip_or_cidr(clean) {
            ips.push(clean.to_string());
        } else {
            let mut found = false;
            if let Some(arp_data) = effective_arp {
                if let Some(ip) = find_ip_for_mac_in_arp(clean, arp_data) {
                    ips.push(ip);
                    found = true;
                }
            }
            if !found {
                if let Some(dev) = cfg.gaming.devices.iter().find(|d| d.mac.eq_ignore_ascii_case(clean)) {
                    if !dev.ip.trim().is_empty() {
                        ips.push(dev.ip.trim().to_string());
                        found = true;
                    }
                }
            }
            if !found {
                if let Some(dev) = cfg.whitelist.devices.iter().find(|d| d.mac.eq_ignore_ascii_case(clean)) {
                    if !dev.ip.trim().is_empty() {
                        ips.push(dev.ip.trim().to_string());
                        found = true;
                    }
                }
            }
            if !found {
                if let Some((_, last_ip)) = cfg
                    .whitelist
                    .excluded_last_ips
                    .iter()
                    .find(|(mac_k, _)| mac_k.eq_ignore_ascii_case(clean))
                {
                    let trimmed = last_ip.trim();
                    if crate::config::is_valid_ip_or_cidr(trimmed) {
                        ips.push(trimmed.to_string());
                    }
                }
            }
        }
    }
    ips.sort();
    ips.dedup();
    ips
}

/// Обновляет `whitelist.excluded_last_ips` по актуальной ARP-таблице для MAC из `excluded_devices` (баг 2).
/// Возвращает `true`, если хотя бы один IP добавлен или изменился (требуется пересборка правил).
pub fn refresh_excluded_whitelist_last_ips(cfg: &mut crate::config::AppConfig, arp_content: &str) -> bool {
    let mut changed = false;
    for item in &cfg.whitelist.excluded_devices {
        let clean = item.trim();
        if clean.is_empty() || crate::config::is_valid_ip_or_cidr(clean) {
            continue;
        }
        if let Some(new_ip) = find_ip_for_mac_in_arp(clean, arp_content) {
            let key = clean.to_lowercase();
            let prev = cfg.whitelist.excluded_last_ips.get(&key).map(|s| s.as_str());
            if prev != Some(new_ip.as_str()) {
                cfg.whitelist.excluded_last_ips.insert(key, new_ip);
                changed = true;
            }
        }
    }
    changed
}
pub fn resolve_excluded_zapret_ips_internal(
    zapret_cfg: &crate::config::ZapretConfig,
    arp_content: Option<&str>,
) -> Vec<String> {
    let mut ips = Vec::new();
    let system_arp = if arp_content.is_none() {
        std::fs::read_to_string("/proc/net/arp").ok()
    } else {
        None
    };
    let effective_arp = arp_content.or(system_arp.as_deref());

    for item in &zapret_cfg.excluded_devices {
        let clean = item.trim();
        if clean.is_empty() {
            continue;
        }
        if crate::config::is_valid_ip_or_cidr(clean) {
            if !ips.contains(&clean.to_string()) {
                ips.push(clean.to_string());
            }
        } else if crate::config::is_valid_mac(clean) {
            if let Some(arp) = effective_arp {
                if let Some(ip) = find_ip_for_mac_in_arp(clean, arp) {
                    if !ips.contains(&ip) {
                        ips.push(ip);
                    }
                }
            }
        }
    }
    ips
}

/// Применение правил режима белых списков:
/// 1. Вставка селектор-группы '🛡️ Whitelist' в proxy-groups:
/// 2. Вставка блока # --- AUTO-WHITELIST-BEGIN --- в rules:
///    - StrictXKeen: только явно добавленные пользователем домены -> DIRECT;
///      прочий трафик клиентов локальных подсетей по умолчанию -> 🛡️ Whitelist.
///    - Standard: встроенный российский каталог и пользовательские домены -> DIRECT;
///      полного перехвата локальных подсетей нет.
/// Явные назначения устройств размещаются перед IPv4 catch-all.
pub fn apply_whitelist_rules(
    content: &str,
    cfg: &crate::config::AppConfig,
    active_tunnel_node: Option<&str>,
) -> String {
    let clean = remove_whitelist_rules(content);
    if !cfg.whitelist.enabled {
        return clean;
    }

    // 1. Формирование и вставка proxy-group 🛡️ Whitelist если она отсутствует
    let existing_groups = parse_existing_group_names(&clean);
    let static_proxies = parse_static_proxy_names(&clean);
    let has_providers_section = clean.contains("proxy-providers:");
    let providers = parse_provider_names(&clean);

    let target_srv = match cfg.whitelist.target_server.as_deref() {
        Some(s) if !s.trim().is_empty() && s.trim() != "follow_failover" => s.trim(),
        _ => match active_tunnel_node {
            Some(node) if !node.trim().is_empty() => node.trim(),
            _ => "Fastest",
        },
    };

    let mut with_group = clean;
    if !existing_groups.iter().any(|g| g == WHITELIST_GROUP_NAME) {
        let lines: Vec<&str> = with_group.lines().collect();
        if let Some(pg_idx) = lines.iter().position(|l| l.trim_end() == "proxy-groups:") {
            let mut pg_out = Vec::with_capacity(lines.len() + 16);
            for (i, line) in lines.iter().enumerate() {
                pg_out.push(line.to_string());
                if i == pg_idx {
                    pg_out.push(WHITELIST_GROUP_BEGIN.to_string());
                    pg_out.push(format!("  - name: '{WHITELIST_GROUP_NAME}'"));
                    pg_out.push("    type: select".to_string());
                    pg_out.push("    proxies:".to_string());

                    let mut group_proxies = Vec::new();
                    let target_is_static_or_group = target_srv == "DIRECT"
                        || target_srv == "REJECT"
                        || existing_groups.iter().any(|g| g == target_srv)
                        || static_proxies.iter().any(|p| p == target_srv)
                        || (!has_providers_section && existing_groups.len() <= 1 && static_proxies.is_empty());

                    if target_is_static_or_group {
                        group_proxies.push(target_srv.to_string());
                    }
                    for p in &["Fastest", "PROXY", "Fallback"] {
                        let exists = existing_groups.iter().any(|g| g == *p)
                            || with_group.contains(&format!("- {p}"))
                            || existing_groups.is_empty();
                        if exists && !group_proxies.iter().any(|x| x == *p) {
                            group_proxies.push(p.to_string());
                        }
                    }
                    for sp in &static_proxies {
                        if !group_proxies.iter().any(|x| x == sp) {
                            group_proxies.push(sp.clone());
                        }
                    }
                    if !group_proxies.iter().any(|x| x == "DIRECT") {
                        group_proxies.push("DIRECT".to_string());
                    }
                    for p in &group_proxies {
                        if p.contains(':') || p.contains('[') || p.contains(']') || p.contains('#') || p.contains('\'') || p.contains('"') {
                            pg_out.push(format!("      - '{}'", p.replace('\'', "''")));
                        } else {
                            pg_out.push(format!("      - {p}"));
                        }
                    }
                    if !providers.is_empty() {
                        pg_out.push("    use:".to_string());
                        for p in &providers {
                            let p_trim = p.trim();
                            if !p_trim.is_empty() {
                                pg_out.push(format!("      - {p_trim}"));
                            }
                        }
                    }
                    pg_out.push(WHITELIST_GROUP_END.to_string());
                }
            }
            with_group = pg_out.join("\n");
        }
    }

    // 2. Формирование правил rules. В StrictXKeen российские каталоги и гео-категории
    // не создают DIRECT-маршрутов: мобильный оператор может не пропускать их напрямую.
    let mut wl_rules = Vec::new();
    wl_rules.push(WHITELIST_BEGIN.to_string());

    let domains_direct = if cfg.whitelist.strategy == crate::config::WhitelistStrategy::StrictXKeen {
        // Вкладка «Мои домены» — явное пользовательское исключение, в отличие от встроенного каталога РФ.
        sanitize_domains(&cfg.whitelist.custom_domains)
    } else {
        get_all_whitelist_domains(&cfg.whitelist)
    };
    for dom in &domains_direct {
        wl_rules.push(format!("  - DOMAIN-SUFFIX,{dom},DIRECT"));
    }

    // При стратегии StrictXKeen маршрутизируем всех клиентов сети в защищенный туннель (Zero-Touch)
    if cfg.whitelist.strategy == crate::config::WhitelistStrategy::StrictXKeen {
        // Баг 39: правила БС всегда указывают на группу 🛡️ Whitelist; выбранный target_server ставится
        // в эту группу в рантайме (switch_group в sync_whitelist_mode_tunnel) — ноды подписок в rules недопустимы.
        let dev_target = WHITELIST_GROUP_NAME;

        // 2.1. Исключенные устройства -> DIRECT
        let excluded_ips = resolve_excluded_whitelist_ips(cfg);
        for ex_ip in &excluded_ips {
            let cidr = if ex_ip.contains('/') {
                ex_ip.clone()
            } else {
                format!("{ex_ip}/32")
            };
            wl_rules.push(format!("  - SRC-IP-CIDR,{cidr},DIRECT"));
        }

        // 2.2. Явно сконфигурированные устройства из whitelist.devices (если есть).
        // BS-PIN: устройство с закреплённой нодой получает СВОЁ правило (выше catch-all
        // 2.4), направленное на его per-device select-группу (group_name_for). Сама
        // группа существует благодаря device_routing-зеркалу в save_whitelist, а её
        // выбор ноды — runtime (mihomo::switch_group при закреплении). Failover
        // управляет только группой Whitelist, поэтому pinned-ноду он не трогает.
        for dev in &cfg.whitelist.devices {
            if dev.enabled && !dev.ip.trim().is_empty() && !excluded_ips.iter().any(|ex| ex == dev.ip.trim()) {
                let ip_trim = dev.ip.trim();
                let cidr = if ip_trim.contains('/') {
                    ip_trim.to_string()
                } else {
                    format!("{ip_trim}/32")
                };
                let pinned = dev.server.as_deref().map(str::trim).filter(|s| !s.is_empty());
                let target = match pinned {
                    Some(_) => group_name_for(ip_trim, dev.name.as_deref().unwrap_or("")),
                    None => dev_target.to_string(),
                };
                let r = format!("  - SRC-IP-CIDR,{cidr},{target}");
                if !wl_rules.contains(&r) {
                    wl_rules.push(r);
                }
            }
        }

        // 2.3. Zero-Touch: если режим БС включён для всей сети (не ограничен списком устройств Devices),
        // все клиенты локальных сетей направляются в выбранный туннель Белых списков.
        if cfg.whitelist.enabled_by != crate::config::WhitelistEnabledBy::Devices {
            for subnet in &["192.168.0.0/16", "10.0.0.0/8", "172.16.0.0/12"] {
                wl_rules.push(format!("  - SRC-IP-CIDR,{subnet},{dev_target}"));
            }
        }
    }

    wl_rules.push(WHITELIST_END.to_string());

    // 3. Вставка в rules: непосредственно над AUTO-DEVICE-RULES (или перед fallback/в конце rules)
    let lines: Vec<&str> = with_group.lines().collect();
    let rules_pos = match lines.iter().position(|l| l.trim_end() == "rules:") {
        Some(pos) => pos,
        None => return with_group,
    };

    let target_insert_idx = if cfg.whitelist.strategy == crate::config::WhitelistStrategy::StrictXKeen {
        // Баг 3: В StrictXKeen catch-all БС ставится сразу после явных назначений устройств (RULES_END)
        // и до блоков Zapret/Gaming, чтобы правила с целью PROXY не стояли выше catch-all (INV-1).
        // Пользовательские списки direct_domains/force_domains накладываются выше с целью DIRECT/Whitelist.
        let managed_end = lines
            .iter()
            .enumerate()
            .filter(|(_, line)| {
                matches!(
                    line.trim(),
                    DEV_DOMAINS_END | DIRECT_END | FORCE_END | RULES_END | ADBLOCK_END
                )
            })
            .map(|(idx, _)| idx + 1)
            .max()
            .unwrap_or(rules_pos + 1);
        managed_end.max(rules_pos + 1)
    } else if let Some(dev_rules_idx) = lines[rules_pos + 1..].iter().position(|l| l.trim() == RULES_BEGIN) {
        rules_pos + 1 + dev_rules_idx
    } else if let Some(fallback_idx) = lines[rules_pos + 1..].iter().position(|l| {
        let t = l.trim();
        t.starts_with("- GEOIP,") || t.starts_with("- MATCH,")
    }) {
        rules_pos + 1 + fallback_idx
    } else {
        rules_pos + 1
    };

    let mut out = Vec::with_capacity(lines.len() + wl_rules.len());
    for (i, line) in lines.iter().enumerate() {
        if i == target_insert_idx {
            for r in &wl_rules {
                out.push(r.clone());
            }
        }
        out.push(line.to_string());
    }
    if target_insert_idx >= lines.len() {
        for r in &wl_rules {
            out.push(r.clone());
        }
    }

    out.join("\n")
}

/// Применение DNS nameserver-policy для доменов режима белых списков.
/// Направляет резолвинг отечественных доменов в локальный DNS роутера (127.0.0.1:53).
pub fn apply_whitelist_dns(
    content: &str,
    cfg: &crate::config::AppConfig,
    local_dns_endpoint: &str,
) -> String {
    let clean = remove_whitelist_dns(content);
    let has_devices_configured = !cfg.whitelist.devices.is_empty();
    let has_active_devices = cfg
        .whitelist
        .devices
        .iter()
        .any(|d| d.enabled && !d.ip.trim().is_empty());

    // Если устройства настроены, но ни одно не активно — не инжектим.
    // Если устройств в списке нет вовсе — режим Zero-Touch на всю сеть: обязательно инжектим.
    if !cfg.whitelist.enabled || (has_devices_configured && !has_active_devices) {
        return clean;
    }

    let endpoint = if local_dns_endpoint.trim().is_empty() {
        "127.0.0.1:53"
    } else {
        local_dns_endpoint.trim()
    };

    let all_domains = get_all_whitelist_domains(&cfg.whitelist);
    let mut dns_block = Vec::with_capacity(all_domains.len() + 2);
    dns_block.push(format!("    {WHITELIST_DNS_BEGIN}"));
    for dom in &all_domains {
        dns_block.push(format!("    '+.{dom}': '{endpoint}'"));
    }
    dns_block.push(format!("    {WHITELIST_DNS_END}"));

    let lines: Vec<&str> = clean.lines().collect();
    if let Some(pos) = lines.iter().position(|l| l.trim() == "nameserver-policy:") {
        let mut out = Vec::with_capacity(lines.len() + dns_block.len());
        for (i, line) in lines.iter().enumerate() {
            out.push(line.to_string());
            if i == pos {
                for entry in &dns_block {
                    out.push(entry.clone());
                }
            }
        }
        return out.join("\n");
    }

    if let Some(dns_pos) = lines.iter().position(|l| l.trim() == "dns:" || l.trim_end() == "dns:") {
        let mut out = Vec::with_capacity(lines.len() + dns_block.len() + 2);
        for (i, line) in lines.iter().enumerate() {
            out.push(line.to_string());
            if i == dns_pos {
                out.push("  nameserver-policy:".to_string());
                for entry in &dns_block {
                    out.push(entry.clone());
                }
            }
        }
        return out.join("\n");
    }

    let mut out = Vec::with_capacity(lines.len() + dns_block.len() + 3);
    for line in lines {
        out.push(line.to_string());
    }
    out.push("dns:".to_string());
    out.push("  enable: true".to_string());
    out.push("  nameserver-policy:".to_string());
    for entry in &dns_block {
        out.push(entry.clone());
    }
    out.join("\n")
}

/// Удаление блоков Canary Probe (селектор-группы и слушателя listeners) из YAML.
pub fn remove_canary_rules(yaml: &str) -> String {
    let mut out = Vec::new();
    let mut skip_group = false;
    let mut skip_listeners = false;

    for line in yaml.lines() {
        let t = line.trim();
        if t == CANARY_GROUP_BEGIN {
            skip_group = true;
            continue;
        }
        if t == CANARY_GROUP_END {
            skip_group = false;
            continue;
        }
        if t == CANARY_LISTENERS_BEGIN {
            skip_listeners = true;
            continue;
        }
        if t == CANARY_LISTENERS_END {
            skip_listeners = false;
            continue;
        }
        if !skip_group && !skip_listeners {
            out.push(line);
        }
    }
    let res = out.join("\n");
    if yaml.ends_with('\n') && !res.is_empty() && !res.ends_with('\n') {
        format!("{res}\n")
    } else {
        res
    }
}
/// Вставка Canary Probe с явным списком провайдеров.
pub fn apply_canary_rules_with_providers(yaml: &str, providers: &[String]) -> String {
    let clean = remove_canary_rules(yaml);

    let static_proxies = parse_static_proxy_names(&clean);
    let parsed_providers = if providers.is_empty() {
        parse_provider_names(&clean)
    } else {
        providers.to_vec()
    };

    let lines: Vec<&str> = clean.lines().collect();

    // 1. Формирование блока listeners:
    let has_existing_listeners = lines.iter().any(|l| l.trim_end() == "listeners:" && !l.starts_with(' '));
    let mut listeners_block = Vec::new();
    listeners_block.push(CANARY_LISTENERS_BEGIN.to_string());
    if !has_existing_listeners {
        listeners_block.push("listeners:".to_string());
    }
    listeners_block.push(format!("  - name: {CANARY_PROBE_INBOUND_NAME}"));
    listeners_block.push("    type: mixed".to_string());
    listeners_block.push(format!("    port: {CANARY_PROBE_PORT}"));
    listeners_block.push(format!("    listen: {CANARY_PROBE_LISTEN}"));
    listeners_block.push(format!("    proxy: '{CANARY_PROBE_GROUP_NAME}'"));
    listeners_block.push(CANARY_LISTENERS_END.to_string());

    // 2. Вставка listeners в YAML
    let with_listeners = if let Some(l_idx) = lines.iter().position(|l| l.trim_end() == "listeners:" && !l.starts_with(' ')) {
        let mut out = Vec::with_capacity(lines.len() + listeners_block.len());
        for (i, line) in lines.iter().enumerate() {
            out.push(line.to_string());
            if i == l_idx {
                out.extend(listeners_block.clone());
            }
        }
        out.join("\n")
    } else {
        // Если секции listeners: нет, вставляем её перед proxy-groups:
        let pg_idx = lines.iter().position(|l| l.trim_end() == "proxy-groups:" && !l.starts_with(' '))
            .unwrap_or(0);
        let mut out = Vec::with_capacity(lines.len() + listeners_block.len() + 1);
        for (i, line) in lines.iter().enumerate() {
            if i == pg_idx {
                out.extend(listeners_block.clone());
            }
            out.push(line.to_string());
        }
        if pg_idx >= lines.len() {
            out.extend(listeners_block.clone());
        }
        out.join("\n")
    };

    // 3. Формирование блока proxy-group '🛡️ Canary Probe'
    let mut pg_block = Vec::new();
    pg_block.push(CANARY_GROUP_BEGIN.to_string());
    pg_block.push(format!("  - name: '{CANARY_PROBE_GROUP_NAME}'"));
    pg_block.push("    type: select".to_string());
    pg_block.push("    proxies:".to_string());
    pg_block.push("      - DIRECT".to_string());
    pg_block.push("      - REJECT".to_string());

    for sp in &static_proxies {
        if sp != "DIRECT" && sp != "REJECT" {
            if sp.contains(':') || sp.contains('[') || sp.contains(']') || sp.contains('\'') || sp.contains('"') || sp.contains('#') {
                pg_block.push(format!("      - '{}'", sp.replace('\'', "''")));
            } else {
                pg_block.push(format!("      - {sp}"));
            }
        }
    }

    let active_providers: Vec<&String> = parsed_providers.iter().filter(|p| !p.trim().is_empty()).collect();
    if !active_providers.is_empty() {
        pg_block.push("    use:".to_string());
        for p in active_providers {
            pg_block.push(format!("      - {p}"));
        }
    }
    pg_block.push(CANARY_GROUP_END.to_string());

    // 4. Вставка proxy-group в секцию proxy-groups:
    let final_lines: Vec<&str> = with_listeners.lines().collect();
    if let Some(pg_pos) = final_lines.iter().position(|l| l.trim_end() == "proxy-groups:" && !l.starts_with(' ')) {
        let mut final_out = Vec::with_capacity(final_lines.len() + pg_block.len());
        for (i, line) in final_lines.iter().enumerate() {
            final_out.push(line.to_string());
            if i == pg_pos {
                final_out.extend(pg_block.clone());
            }
        }
        let res = final_out.join("\n");
        if yaml.ends_with('\n') && !res.ends_with('\n') {
            format!("{res}\n")
        } else {
            res
        }
    } else {
        with_listeners
    }
}


/// Баг 32: Разрешение IP-адресов устройств, исключённых из Zapret (по IP или MAC).
#[cfg(test)]
pub fn resolve_excluded_zapret_ips(zapret_cfg: &crate::config::ZapretConfig) -> Vec<String> {
    resolve_excluded_zapret_ips_internal(zapret_cfg, None)
}

/// Вставка селектор-группы '🛡️ Canary Probe' и локального слушателя 127.0.0.1:7894 в config.yaml.
/// Включает все статические прокси и подписки (use:), а также DIRECT и REJECT.
#[cfg(test)]
pub fn apply_canary_rules(yaml: &str) -> String {
    let providers = parse_provider_names(yaml);
    apply_canary_rules_with_providers(yaml, &providers)
}
