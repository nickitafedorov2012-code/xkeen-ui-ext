use std::collections::BTreeMap;
use super::*;

/// Удаление доменных блоков (DIRECT/FORCE/DEVICE-DOMAINS) из YAML.
pub fn remove_domain_blocks(yaml: &str) -> String {
    let has_direct_end = yaml.contains(DIRECT_END);
    let has_force_end = yaml.contains(FORCE_END);
    let has_dev_end = yaml.contains(DEV_DOMAINS_END);

    let mut out = Vec::new();
    let mut in_direct = false;
    let mut in_force = false;
    let mut in_dev = false;

    for line in yaml.lines() {
        let t = line.trim();
        if t == DIRECT_BEGIN {
            if has_direct_end { in_direct = true; }
            continue;
        }
        if t == DIRECT_END {
            in_direct = false;
            continue;
        }
        if t == FORCE_BEGIN {
            if has_force_end { in_force = true; }
            continue;
        }
        if t == FORCE_END {
            in_force = false;
            continue;
        }
        if t == DEV_DOMAINS_BEGIN {
            if has_dev_end { in_dev = true; }
            continue;
        }
        if t == DEV_DOMAINS_END {
            in_dev = false;
            continue;
        }
        if !in_direct && !in_force && !in_dev {
            out.push(line);
        }
    }
    out.join("\n")
}

/// Вставка доменных правил в rules: (сразу после строки rules:, чтобы они имели
/// приоритет над остальными правилами). Пустые списки = блоки удаляются.
pub fn apply_domain_rules(
    yaml: &str,
    direct: &[String],
    force: &[String],
    device_domains: &std::collections::BTreeMap<String, Vec<crate::config::DeviceDomainRule>>,
) -> Result<String, String> {
    apply_domain_rules_with_priority(yaml, direct, force, device_domains, false)
}

/// Apply domain settings in the same order used by the full routing generator.
/// StrictXKeen gives the two global settings lists priority over per-device domains.
pub fn apply_domain_rules_for_config(
    yaml: &str,
    cfg: &crate::config::AppConfig,
    force: &[String],
) -> Result<String, String> {
    let settings_first = cfg.whitelist.enabled
        && cfg.whitelist.strategy == crate::config::WhitelistStrategy::StrictXKeen;
    apply_domain_rules_with_priority(
        yaml,
        &cfg.direct_domains,
        force,
        &cfg.device_domain_rules,
        settings_first,
    )
}

pub(crate) fn apply_domain_rules_with_priority(
    yaml: &str,
    direct: &[String],
    force: &[String],
    device_domains: &std::collections::BTreeMap<String, Vec<crate::config::DeviceDomainRule>>,
    settings_first: bool,
) -> Result<String, String> {
    apply_domain_rules_with_priority_for_target(yaml, direct, force, device_domains, settings_first, None)
}

pub(crate) fn apply_domain_rules_with_priority_for_target(
    yaml: &str,
    direct: &[String],
    force: &[String],
    device_domains: &std::collections::BTreeMap<String, Vec<crate::config::DeviceDomainRule>>,
    settings_first: bool,
    whitelist_override_target: Option<&str>,
) -> Result<String, String> {
    crate::routing::validate_marker_pair(yaml, DIRECT_BEGIN, DIRECT_END)?;
    crate::routing::validate_marker_pair(yaml, FORCE_BEGIN, FORCE_END)?;
    crate::routing::validate_marker_pair(yaml, DEV_DOMAINS_BEGIN, DEV_DOMAINS_END)?;
    let content = remove_domain_blocks(yaml);
    let direct = sanitize_domains(direct);
    let direct_set: std::collections::HashSet<String> = direct.iter().cloned().collect();
    let force: Vec<String> = sanitize_domains(force)
        .into_iter()
        .filter(|d| !direct_set.contains(d))
        .collect();
    let has_dev_domains = !device_domains.is_empty() && device_domains.values().any(|v| !v.is_empty());

    if direct.is_empty() && force.is_empty() && !has_dev_domains {
        return Ok(content);
    }
    let lines: Vec<&str> = content.lines().collect();
    let rules_idx = lines
        .iter()
        .position(|l| l.trim_end() == "rules:")
        .ok_or("В config.yaml нет секции rules:")?;

    let mut out = Vec::with_capacity(lines.len() + direct.len() + force.len() + 12);
    let mut device_block = Vec::new();
    if has_dev_domains {
        device_block.push(DEV_DOMAINS_BEGIN.to_string());
        for (ip, rules) in device_domains {
            let cidr = format_src_ip_cidr(ip);
            for r in rules {
                let dom = r.domain.trim().to_lowercase();
                let raw_target = r.target.trim();
                let target = match whitelist_override_target {
                    Some(wl_t)
                        if !raw_target.eq_ignore_ascii_case("DIRECT")
                            && !raw_target.eq_ignore_ascii_case("REJECT")
                            && !raw_target.eq_ignore_ascii_case("REJECT-DROP") =>
                    {
                        wl_t
                    }
                    _ => raw_target,
                };
                // Баг 119: домен и цель не должны ломать AND-правило; несуществующую цель
                // заменяет sanitize_rule_targets в конце apply_routing (INV-4)
                if is_valid_rule_domain(&dom) && is_safe_rule_target(target) {
                    device_block.push(format!(
                        "  - AND,((SRC-IP-CIDR,{cidr}),(DOMAIN-SUFFIX,{dom})),{target},no-resolve"
                    ));
                }
            }
        }
        device_block.push(DEV_DOMAINS_END.to_string());
    }

    let mut direct_block = Vec::new();
    if !direct.is_empty() {
        direct_block.push(DIRECT_BEGIN.to_string());
        for d in &direct {
            direct_block.push(format!("  - DOMAIN-SUFFIX,{d},DIRECT"));
        }
        direct_block.push(DIRECT_END.to_string());
    }

    let mut force_block = Vec::new();
    if !force.is_empty() {
        let proxy_target = whitelist_override_target
            .map(str::to_string)
            .unwrap_or_else(|| find_proxy_target_group(&content));
        force_block.push(FORCE_BEGIN.to_string());
        for d in &force {
            if !FLOW_DOMAINS.iter().any(|g| g.eq_ignore_ascii_case(d)) {
                force_block.push(format!("  - DOMAIN-SUFFIX,{d},{proxy_target}"));
            }
        }
        force_block.push(FORCE_END.to_string());
    }

    for (i, line) in lines.iter().enumerate() {
        out.push(line.to_string());
        if i == rules_idx {
            // В strict_x_keen глобальные списки из настроек имеют высший приоритет,
            // затем идут персональные доменные правила устройств. В остальных режимах
            // сохраняется прежний порядок, где персональные правила стоят первыми.
            if settings_first {
                out.extend(direct_block.clone());
                out.extend(force_block.clone());
                out.extend(device_block.clone());
            } else {
                out.extend(device_block.clone());
                out.extend(direct_block.clone());
                out.extend(force_block.clone());
            }
        }
    }
    Ok(out.join("\n"))
}

/// Нормализует IP или CIDR строку для правила SRC-IP-CIDR (IPv4 -> /32, IPv6 -> /128, без дублирования /32/32).
pub fn format_src_ip_cidr(ip: &str) -> String {
    let clean = ip.trim();
    if let Some((addr_str, prefix_str)) = clean.split_once('/') {
        if let Ok(addr) = addr_str.trim().parse::<std::net::IpAddr>() {
            if let Ok(prefix) = prefix_str.trim().parse::<u8>() {
                if (addr.is_ipv4() && prefix <= 32) || (addr.is_ipv6() && prefix <= 128) {
                    return format!("{addr}/{prefix}");
                }
            }
        }
    }
    if let Ok(addr) = clean.parse::<std::net::IpAddr>() {
        let prefix = if addr.is_ipv6() { 128 } else { 32 };
        return format!("{addr}/{prefix}");
    }
    let prefix = if clean.contains(':') { "128" } else { "32" };
    format!("{clean}/{prefix}")
}

/// Строка правила для устройства (IPv4 -> /32, IPv6 -> /128).
pub fn rule_line(ip: &str, group_name: &str) -> String {
    // IPv4 -> /32, IPv6 -> /128
    let cidr = format_src_ip_cidr(ip);
    format!("  - SRC-IP-CIDR,{cidr},{group_name}")
}

pub(crate) fn extract_block<'a>(yaml: &'a str, begin: &str, end: &str) -> Option<&'a str> {
    let p1 = yaml.find(begin)? + begin.len();
    let p2 = yaml[p1..].find(end)? + p1;
    Some(&yaml[p1..p2])
}

/// Парсинг существующих групп: ip → исходный YAML-текст группы.
pub fn parse_groups(yaml: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Some(block) = extract_block(yaml, GROUPS_BEGIN, GROUPS_END) else {
        return out;
    };
    let re = groups_name_re();
    let mut cur_name: Option<String> = None;
    let mut cur_lines: Vec<&str> = Vec::new();
    for line in block.split('\n') {
        match re.captures(line) {
            Some(c) => {
                if let Some(n) = cur_name.take() {
                    if let Some(ip) = ip_key_from_group(&n) {
                        out.insert(ip, cur_lines.join("\n"));
                    }
                }
                cur_name = Some(c.get(1).map(|m| m.as_str().to_string()).unwrap_or_default());
                cur_lines = vec![line];
            }
            None => {
                if cur_name.is_some() {
                    cur_lines.push(line);
                }
            }
        }
    }
    if let Some(n) = cur_name {
        if let Some(ip) = ip_key_from_group(&n) {
            out.insert(ip, cur_lines.join("\n"));
        }
    }
    out
}

/// Парсинг существующих правил: ip → исходная строка правила.
pub fn parse_rules(yaml: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Some(block) = extract_block(yaml, RULES_BEGIN, RULES_END) else {
        return out;
    };
    let re = rules_cidr_re();
    for line in block.split('\n') {
        if let Some(c) = re.captures(line) {
            let base_ip = c.get(1).map(|m| m.as_str().trim().to_string()).unwrap_or_default();
            let prefix = c.get(2).map(|m| m.as_str().trim());
            if !base_ip.is_empty() {
                let trimmed_line = line.trim_end().to_string();
                if let Some(p) = prefix {
                    if p != "32" && p != "128" {
                        out.insert(format!("{base_ip}/{p}"), trimmed_line.clone());
                    }
                }
                out.insert(base_ip, trimmed_line);
            }
        }
    }
    out
}

/// Удаление AUTO-блоков из YAML (для последующей вставки объединённых).
pub fn remove_blocks(yaml: &str) -> String {
    let mut out = yaml.to_string();
    for (begin, end) in [(RULES_BEGIN, RULES_END), (GROUPS_BEGIN, GROUPS_END)] {
        if let (Some(p1), Some(p2rel)) = (out.find(begin), out.find(end)) {
            let p2 = p2rel + end.len();
            if p1 < p2 {
                out = format!("{}{}", &out[..p1], &out[p2..]);
            }
        }
    }
    out
}

/// BS-PIN: IP устройств с закреплённой нодой режима БС (server непустой).
/// Группа такого устройства защищена от удаления в apply_assignments —
/// на неё ссылается правило whitelist-блока, и снос группы ломает конфиг.
pub fn pinned_whitelist_ips(cfg: &crate::config::AppConfig) -> Vec<String> {
    cfg.whitelist
        .devices
        .iter()
        .filter(|d| {
            d.enabled
                && !d.ip.trim().is_empty()
                && !d.server.as_deref().map(str::trim).unwrap_or("").is_empty()
        })
        .map(|d| d.ip.trim().to_string())
        .collect()
}

/// Merge-семантика десктопа: существующие правила сохраняются, применяются только
/// переданные назначения (server=None/"default" — снять). Возвращает новый YAML.
/// pinned_ips: устройства БС-закрепления — их группа не удаляется при снятии
/// назначения (правило whitelist-блока продолжает ссылаться на неё), а их
/// device-правило убирается как недостижимое (приоритет у whitelist-блока).
pub fn apply_assignments(
    yaml: &str,
    assignments: &[Assignment],
    providers: &[String],
    pinned_ips: &[String],
) -> Result<String, String> {
    let mut groups_by_ip = parse_groups(yaml);
    let mut rules_by_ip = parse_rules(yaml);

    for a in assignments {
        let ip = a.ip.trim();
        if ip.is_empty() || ip == "0.0.0.0" {
            continue;
        }
        let remove = match &a.server {
            None => true,
            Some(s) => s.trim().is_empty() || s.trim() == "default",
        };
        if remove {
            if pinned_ips.iter().any(|p| p == ip) {
                // BS-PIN: группу сохраняем как есть (на неё ссылается whitelist-правило
                // с именем из whitelist.devices) — имя из assignment не используем,
                // чтобы не переименовать группу и не сломать ссылку. Если группы
                // ещё нет — создаём с именем из assignment.
                if !groups_by_ip.contains_key(ip) {
                    let gname = group_name_for(ip, &a.name);
                    groups_by_ip.insert(ip.to_string(), group_yaml(&gname, providers));
                }
                // недостижимое device-правило убираем (приоритет у whitelist-блока)
                rules_by_ip.remove(ip);
            } else {
                groups_by_ip.remove(ip);
                rules_by_ip.remove(ip);
            }
            continue;
        }
        let gname = group_name_for(ip, &a.name);
        if pinned_ips.iter().any(|p| p == ip) {
            // BS-PIN активен: имя группы менять нельзя (ссылка из whitelist-правила),
            // существующую группу сохраняем; device-правило не нужно — трафик
            // перехватывает whitelist-блок выше
            groups_by_ip.entry(ip.to_string())
                .or_insert_with(|| group_yaml(&gname, providers));
            rules_by_ip.remove(ip);
        } else {
            groups_by_ip.insert(ip.to_string(), group_yaml(&gname, providers));
            rules_by_ip.insert(ip.to_string(), rule_line(ip, &gname));
        }
    }

    let mut content = remove_blocks(yaml);

    let groups_sorted: Vec<String> = groups_by_ip.values().cloned().collect();
    if !groups_sorted.is_empty() {
        let block = format!("{GROUPS_BEGIN}\n{}\n{GROUPS_END}\n", groups_sorted.join("\n"));
        let pos = content
            .find("proxy-groups:")
            .ok_or("В config.yaml нет секции proxy-groups:")?
            + "proxy-groups:".len();
        let prefix = if content[..pos].ends_with('\n') { "" } else { "\n" };
        content = format!("{}{prefix}{}\n{}", &content[..pos], block, &content[pos..]);
    }

    let rules_sorted: Vec<String> = rules_by_ip
        .iter()
        .filter(|(ip, _)| groups_by_ip.contains_key(*ip))
        .map(|(_, line)| line.clone())
        .collect();
    if !rules_sorted.is_empty() {
        let block = format!("{RULES_BEGIN}\n{}\n{RULES_END}\n", rules_sorted.join("\n"));
        let pos = content.find("rules:").ok_or("В config.yaml нет секции rules:")? + "rules:".len();
        let prefix = if content[..pos].ends_with('\n') { "" } else { "\n" };
        content = format!("{}{prefix}{}\n{}", &content[..pos], block, &content[pos..]);
    }

    Ok(content)
}

/// Экранирование имени сервера для regex (exclude-filter использует Go regexp).
pub fn regex_escape(s: &str) -> String {
    regex_lite::escape(s)
}

/// Подстрочный OR-regex для exclude-filter провайдеров (надёжнее к эмодзи/вариант-селекторам).
fn filter_line(ignore: &[String]) -> String {
    ignore.iter().map(|s| regex_escape(s.trim())).collect::<Vec<_>>().join("|")
}

/// Имена статических прокси из секции proxies верхнего уровня.
pub fn parse_static_proxy_names(yaml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_section = false;
    for line in yaml.lines() {
        if line.trim_end() == "proxies:" && !line.starts_with(' ') {
            in_section = true;
            continue;
        }
        if in_section {
            if !line.starts_with(' ') && !line.trim().is_empty() {
                break;
            }
            let t = line.trim();
            if let Some(rest) = t.strip_prefix("- name:") {
                let name = rest.trim().trim_matches('\'').trim_matches('"').to_string();
                if !name.is_empty() {
                    out.push(name);
                }
            }
        }
    }
    out
}

fn push_proxies_block(out: &mut Vec<String>, names: &[String]) {
    out.push("    proxies:".to_string());
    for n in names {
        out.push(format!("      - '{}'", n.replace('\'', "''")));
    }
}

/// Применение игнор-листа к группам Fastest/Fallback:
/// - статические прокси (секция proxies), чьё имя содержит любой из ignore (подстрочно,
///   без учёта регистра), исключаются: include-all заменяется на явный proxies-список;
/// - для провайдеров (use) ставится подстрочный exclude-filter;
/// - пустой ignore → полное восстановление include-all и удаление наших вставок.
pub fn apply_ignore_to_groups(yaml: &str, ignore: &[String]) -> Result<String, String> {
    let static_names = parse_static_proxy_names(yaml);
    let ig_lower: Vec<String> = ignore.iter().map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()).collect();
    let use_explicit = !ig_lower.is_empty();
    let kept: Vec<String> = static_names
        .iter()
        .filter(|n| {
            let nl = n.to_lowercase();
            !ig_lower.iter().any(|ig| nl.contains(ig.as_str()))
        })
        .cloned()
        .collect();

    let filter = if ig_lower.is_empty() { String::new() } else { filter_line(ignore) };
    let rendered = |indent: &str| format!("{indent}exclude-filter: '{}'", filter.replace('\'', "''"));

    let mut out: Vec<String> = Vec::with_capacity(yaml.lines().count() + 16);
    let mut in_target = false;
    let mut filter_done = false;
    let mut proxies_done = false;
    let mut include_seen = false;
    let mut in_old_proxies_list = false;

    for line in yaml.lines() {
        let indent_len = line.len() - line.trim_start().len();
        let trimmed = line.trim();
        let is_group_start = trimmed.starts_with("- name:") && indent_len <= 4;
        let is_target_start = is_group_start && matches!(
            trimmed,
            "- name: Fastest"
                | "- name: Fallback"
                | "- name: 'Fastest'"
                | "- name: 'Fallback'"
                | "- name: \"Fastest\""
                | "- name: \"Fallback\""
        );

        if is_target_start {
            if in_target {
                if !filter_done && !filter.is_empty() {
                    out.push(rendered("    "));
                }
                if use_explicit && !proxies_done && !kept.is_empty() {
                    push_proxies_block(&mut out, &kept);
                }
                if !use_explicit && !include_seen {
                    out.push("    include-all: true".to_string());
                }
            }
            in_target = true;
            filter_done = false;
            proxies_done = false;
            include_seen = false;
            in_old_proxies_list = false;
            out.push(line.to_string());
            continue;
        }

        if in_target {
            let is_new_group = is_group_start;
            let is_new_section = !line.starts_with(' ') && !trimmed.is_empty();
            if is_new_group || is_new_section {
                if !filter_done && !filter.is_empty() {
                    out.push(rendered("    "));
                }
                if use_explicit && !proxies_done && !kept.is_empty() {
                    push_proxies_block(&mut out, &kept);
                }
                if !use_explicit && !include_seen {
                    out.push("    include-all: true".to_string());
                }
                in_target = false;
                in_old_proxies_list = false;
                out.push(line.to_string());
                continue;
            }

            if in_old_proxies_list {
                if trimmed.starts_with("- ") && indent_len > 2 {
                    continue;
                }
                in_old_proxies_list = false;
            }

            if trimmed.starts_with("exclude-filter:") {
                if !filter.is_empty() && !filter_done {
                    out.push(rendered("    "));
                    filter_done = true;
                }
                continue;
            }

            if trimmed == "proxies:" && indent_len >= 2 {
                if use_explicit && !proxies_done && !kept.is_empty() {
                    push_proxies_block(&mut out, &kept);
                    proxies_done = true;
                }
                in_old_proxies_list = true;
                continue;
            }

            if trimmed == "include-all: true" {
                include_seen = true;
                if use_explicit {
                    if !proxies_done && !kept.is_empty() {
                        push_proxies_block(&mut out, &kept);
                        proxies_done = true;
                    }
                    continue;
                }
                out.push(line.to_string());
                if !filter.is_empty() && !filter_done {
                    out.push(rendered("    "));
                    filter_done = true;
                }
                continue;
            }

            out.push(line.to_string());
        } else {
            out.push(line.to_string());
        }
    }
    if in_target {
        if !filter_done && !filter.is_empty() {
            out.push(rendered("    "));
        }
        if use_explicit && !proxies_done && !kept.is_empty() {
            push_proxies_block(&mut out, &kept);
        }
        if !use_explicit && !include_seen {
            out.push("    include-all: true".to_string());
        }
    }
    Ok(out.join("\n"))
}
/// Очистка и запрет exclude-filter у провайдеров (proxy-providers).
/// По железному правилу проекта: exclude-filter в провайдерах категорически запрещён,
/// чтобы ядро не срезало серверы Hysteria2 и RU с 13 до 10.
pub fn apply_ignore_to_providers(yaml: &str, _ignore: &[String], saved: &mut std::collections::BTreeMap<String, String>) -> String {
    saved.clear();
    let mut out: Vec<String> = Vec::with_capacity(yaml.lines().count());
    let mut in_providers = false;

    for line in yaml.lines() {
        let trimmed = line.trim();
        let is_top = !line.starts_with(' ') && !trimmed.is_empty();

        if line.trim_end() == "proxy-providers:" {
            in_providers = true;
            out.push(line.to_string());
            continue;
        }
        if in_providers && is_top {
            in_providers = false;
            out.push(line.to_string());
            continue;
        }
        if in_providers {
            if trimmed.starts_with("exclude-filter:") {
                continue;
            }
            out.push(line.to_string());
        } else {
            out.push(line.to_string());
        }
    }
    out.join("\n")
}

