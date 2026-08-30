//! Раздельная маршрутизация per-device: AUTO-DEVICE-GROUPS / AUTO-DEVICE-RULES блоки
//! в /opt/etc/mihomo/config.yaml. Формат блоков 1:1 с десктопным KeeneticPolicyManager
//! (общее хранилище правил ПК/Android/веб-версий).

use regex_lite::Regex;
use std::collections::BTreeMap;
use std::sync::LazyLock;

/// Regex'ы парсинга компилируются один раз (LazyLock), а не на каждый вызов
/// parse_groups/parse_rules — экономия на горячем пути /api/routing.
fn groups_name_re() -> &'static Regex {
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s{2}- name: '(.+?)'\s*$").unwrap());
    &RE
}

fn rules_cidr_re() -> &'static Regex {
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*- SRC-IP-CIDR,(.+?)/32,").unwrap());
    &RE
}

pub const GROUPS_BEGIN: &str = "# --- AUTO-DEVICE-GROUPS-BEGIN ---";
pub const GROUPS_END: &str = "# --- AUTO-DEVICE-GROUPS-END ---";
pub const RULES_BEGIN: &str = "# --- AUTO-DEVICE-RULES-BEGIN ---";
pub const RULES_END: &str = "# --- AUTO-DEVICE-RULES-END ---";

#[derive(Clone, Debug)]
pub struct Assignment {
    pub ip: String,
    pub name: String,
    /// None или "default" — снять назначение устройства.
    pub server: Option<String>,
}

/// 'Big PC 192_168_2_118' → '192.168.2.118'; 'DEV_aa_bb_cc_dd_ee_ff' → MAC-вид.
/// None — имя группы не кодирует устройство.
pub fn ip_key_from_group(gname: &str) -> Option<String> {
    let mut token = gname.trim().split(' ').next_back().unwrap_or("");
    if let Some(stripped) = token.strip_prefix("DEV_") {
        token = stripped;
    }
    let cand = token.replace('_', ".");
    let parts: Vec<&str> = cand.split('.').collect();
    if parts.len() == 4 && parts.iter().all(|p| p.parse::<u8>().is_ok()) {
        return Some(cand);
    }
    if token.contains('_') {
        Some(token.replace('_', ":"))
    } else {
        None
    }
}

/// Имя группы устройства: '{чистое имя} {ip_с_подчёркиваниями}' или 'DEV_{ip}'.
pub fn group_name_for(ip: &str, name: &str) -> String {
    let safe_ip = ip.replace('.', "_").replace(':', "_");
    let clean: String = name
        .chars()
        .filter(|c| !matches!(c, '\r' | '\n' | ',' | '\'' | '"' | '#'))
        .collect();
    let clean = clean.trim();
    if clean.is_empty() {
        format!("DEV_{safe_ip}")
    } else {
        format!("{clean} {safe_ip}")
    }
}

/// YAML-текст select-группы устройства. Провайдеры (use:) передаются снаружи —
/// на чужом железе имена провайдеров свои (или их нет вовсе).
pub fn group_yaml(group_name: &str, providers: &[String]) -> String {
    let base = format!(
        "  - name: '{group_name}'\n    type: select\n    proxies:\n      - Fastest\n      - Fallback"
    );
    let mut use_block = String::new();
    for p in providers {
        let p = p.trim();
        if p.is_empty() {
            continue;
        }
        if use_block.is_empty() {
            use_block.push_str("\n    use:");
        }
        use_block.push_str(&format!("\n      - {p}"));
    }
    format!("{base}{use_block}")
}

/// Имена proxy-providers верхнего уровня из config.yaml (для универсальности:
/// на другом железе имена провайдеров свои).
pub fn parse_provider_names(yaml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_providers = false;
    for line in yaml.lines() {
        let trimmed = line.trim();
        if line.trim_end() == "proxy-providers:" {
            in_providers = true;
            continue;
        }
        if in_providers {
            let is_top = !line.starts_with(' ') && !trimmed.is_empty();
            if is_top {
                break;
            }
            // имя провайдера: строка "  name:" (ровно 2 пробела, ключ мапы)
            if line.starts_with("  ") && !line.starts_with("   ") && trimmed.ends_with(':') && !trimmed.starts_with('-') {
                out.push(trimmed.trim_end_matches(':').to_string());
            }
        }
    }
    out
}

/// Домены: очистка и нормализация (нижний регистр, без пробелов/протоколов/путей).
pub fn sanitize_domains(list: &[String]) -> Vec<String> {
    let mut out: Vec<String> = list
        .iter()
        .map(|d| {
            d.trim()
                .to_lowercase()
                .trim_start_matches("https://")
                .trim_start_matches("http://")
                .trim_start_matches("www.")
                .to_string()
        })
        .map(|d| d.split('/').next().unwrap_or("").trim().to_string())
        .filter(|d| !d.is_empty() && d.contains('.'))
        .collect();
    out.sort();
    out.dedup();
    out
}

// --- Доменные списки: AUTO-DIRECT / AUTO-FORCE блоки в rules ---

pub const DIRECT_BEGIN: &str = "# --- AUTO-DIRECT-BEGIN ---";
pub const DIRECT_END: &str = "# --- AUTO-DIRECT-END ---";
pub const FORCE_BEGIN: &str = "# --- AUTO-FORCE-BEGIN ---";
pub const FORCE_END: &str = "# --- AUTO-FORCE-END ---";

/// Удаление доменных блоков (DIRECT/FORCE) из YAML.
pub fn remove_domain_blocks(yaml: &str) -> String {
    let mut out = yaml.to_string();
    for (begin, end) in [(DIRECT_BEGIN, DIRECT_END), (FORCE_BEGIN, FORCE_END)] {
        if let (Some(p1), Some(p2rel)) = (out.find(begin), out.find(end)) {
            let p2 = p2rel + end.len();
            if p1 < p2 {
                out = format!("{}{}", &out[..p1], &out[p2..]);
            }
        }
    }
    out
}

fn domain_block(begin: &str, end: &str, domains: &[String], target: &str) -> String {
    if domains.is_empty() {
        return String::new();
    }
    let mut out = format!("{begin}\n");
    for d in domains {
        out.push_str(&format!("  - DOMAIN-SUFFIX,{d},{target}\n"));
    }
    out.push_str(end);
    out.push('\n');
    out
}

/// Вставка доменных правил в rules: (сразу после строки rules:, чтобы они имели
/// приоритет над остальными правилами). Пустые списки = блоки удаляются.
pub fn apply_domain_rules(yaml: &str, direct: &[String], force: &[String]) -> Result<String, String> {
    let content = remove_domain_blocks(yaml);
    let direct = sanitize_domains(direct);
    let force = sanitize_domains(force);
    if direct.is_empty() && force.is_empty() {
        return Ok(content);
    }
    // Ищем строку "rules:" верхнего уровня.
    let mut insert_pos: Option<usize> = None;
    let mut offset = 0usize;
    for line in content.lines() {
        if line.trim_end() == "rules:" {
            insert_pos = Some(offset + line.len());
            break;
        }
        offset += line.len() + 1; // +1 за \n
    }
    let pos = insert_pos.ok_or("В config.yaml нет секции rules:")?;
    let block = format!(
        "\n{}{}",
        domain_block(DIRECT_BEGIN, DIRECT_END, &direct, "DIRECT"),
        domain_block(FORCE_BEGIN, FORCE_END, &force, "PROXY")
    );
    Ok(format!("{}{}{}", &content[..pos], block, &content[pos..]))
}

/// Строка правила для устройства.
pub fn rule_line(ip: &str, group_name: &str) -> String {
    format!("  - SRC-IP-CIDR,{ip}/32,{group_name}")
}

fn extract_block<'a>(yaml: &'a str, begin: &str, end: &str) -> Option<&'a str> {
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
            let ip = c.get(1).map(|m| m.as_str().trim().to_string()).unwrap_or_default();
            if !ip.is_empty() {
                out.insert(ip, line.trim_end().to_string());
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

/// Merge-семантика десктопа: существующие правила сохраняются, применяются только
/// переданные назначения (server=None/"default" — снять). Возвращает новый YAML.
pub fn apply_assignments(yaml: &str, assignments: &[Assignment], providers: &[String]) -> Result<String, String> {
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
            groups_by_ip.remove(ip);
            rules_by_ip.remove(ip);
            continue;
        }
        let server = a.server.clone().unwrap_or_default();
        let gname = group_name_for(ip, &a.name);
        groups_by_ip.insert(ip.to_string(), group_yaml(&gname, providers));
        rules_by_ip.insert(ip.to_string(), rule_line(ip, &gname));
        let _ = server;
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
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        if matches!(c, '\\' | '.' | '+' | '*' | '?' | '(' | ')' | '|' | '[' | ']' | '{' | '}' | '^' | '$') {
            out.push('\\');
        }
        out.push(c);
    }
    out
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
        let trimmed = line.trim();
        let is_target_start = matches!(
            trimmed,
            "- name: Fastest" | "- name: Fallback" | "- name: 'Fastest'" | "- name: 'Fallback'"
        ) && !line.starts_with("    ");

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
            let is_new_group = trimmed.starts_with("- name:") && !line.starts_with("    ");
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
                if line.starts_with("      - ") {
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

            if trimmed == "proxies:" && line.starts_with("    ") {
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
/// Обновление exclude-filter провайдеров (proxy-providers): добавление игнор-подстрок.
/// saved хранит оригинальные фильтры для восстановления при очистке игнор-листа.
pub fn apply_ignore_to_providers(yaml: &str, ignore: &[String], saved: &mut std::collections::BTreeMap<String, String>) -> String {
    let ig: Vec<String> = ignore.iter().map(|s| s.trim()).filter(|s| !s.is_empty()).map(|s| s.to_string()).collect();
    let mut out: Vec<String> = Vec::with_capacity(yaml.lines().count() + 4);
    let mut in_providers = false;
    let mut cur_provider: Option<String> = None;

    for line in yaml.lines() {
        let trimmed = line.trim();
        let is_top = !line.starts_with(' ') && !trimmed.is_empty();

        if line.trim_end() == "proxy-providers:" {
            in_providers = true;
            cur_provider = None;
            out.push(line.to_string());
            continue;
        }
        if in_providers && is_top {
            in_providers = false;
            cur_provider = None;
            out.push(line.to_string());
            continue;
        }
        if in_providers {
            // имя провайдера: строка "  name:" (ровно 2 пробела, ключ мапы)
            if line.starts_with("  ") && !line.starts_with("   ") && trimmed.ends_with(':') && !trimmed.starts_with('-') {
                cur_provider = Some(trimmed.trim_end_matches(':').to_string());
                out.push(line.to_string());
                continue;
            }
            if trimmed.starts_with("exclude-filter:") {
                if let Some(p) = &cur_provider {
                    if ig.is_empty() {
                        if let Some(orig) = saved.get(p) {
                            out.push(format!("    exclude-filter: \"{}\"", orig));
                        } else {
                            out.push(line.to_string());
                        }
                        continue;
                    }
                    let orig = saved.entry(p.clone()).or_insert_with(|| extract_filter_value(trimmed));
                    let mut parts: Vec<String> = orig.split('|').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                    for i in &ig {
                        if !parts.iter().any(|p2| p2.eq_ignore_ascii_case(i)) {
                            parts.push(i.clone());
                        }
                    }
                    out.push(format!("    exclude-filter: \"{}\"", parts.join("|")));
                    continue;
                }
            }
            out.push(line.to_string());
        } else {
            out.push(line.to_string());
        }
    }
    out.join("\n")
}

fn extract_filter_value(trimmed: &str) -> String {
    let v = trimmed.strip_prefix("exclude-filter:").unwrap_or("").trim();
    let v = v.strip_prefix('"').unwrap_or(v);
    let v = v.strip_suffix('"').unwrap_or(v);
    let v = v.strip_prefix('\'').unwrap_or(v);
    v.strip_suffix('\'').unwrap_or(v).to_string()
}
#[cfg(test)]
mod tests {
    use super::*;

    const BASE_YAML: &str = "port: 7890\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies:\n      - Fastest\nrules:\n  - GEOIP,RU,DIRECT\n  - MATCH,PROXY\n";

    #[test]
    fn add_assignment_creates_blocks() {
        let out = apply_assignments(
            BASE_YAML,
            &[Assignment { ip: "192.168.2.118".into(), name: "Big PC".into(), server: Some("рџ‡©рџ‡Є DE".into()) }],
            &["geodema".to_string(), "geodema2".to_string()],
        )
        .unwrap();
        assert!(out.contains(GROUPS_BEGIN));
        assert!(out.contains("- name: 'Big PC 192_168_2_118'"));
        assert!(out.contains("SRC-IP-CIDR,192.168.2.118/32,Big PC 192_168_2_118"));
        assert!(out.contains("- geodema2"), "use: провайдеры из параметра");
        // блоки вставлены сразу после секций
        let gpos = out.find("proxy-groups:").unwrap();
        let bpos = out.find(GROUPS_BEGIN).unwrap();
        assert!(bpos > gpos && bpos - gpos < 20);
        let rpos = out.find("rules:").unwrap();
        let rbpos = out.find(RULES_BEGIN).unwrap();
        assert!(rbpos > rpos && rbpos - rpos < 20);
        // исходные правила не тронуты
        assert!(out.contains("GEOIP,RU,DIRECT"));
    }

    #[test]
    fn merge_preserves_other_devices() {
        let with_one = apply_assignments(
            BASE_YAML,
            &[Assignment { ip: "10.0.0.5".into(), name: "Phone".into(), server: Some("X".into()) }],
            &[],
        )
        .unwrap();
        // добавляем второе устройство — первое должно сохраниться
        let with_two = apply_assignments(
            &with_one,
            &[Assignment { ip: "10.0.0.6".into(), name: "TV".into(), server: Some("Y".into()) }],
            &[],
        )
        .unwrap();
        assert!(with_two.contains("Phone 10_0_0_5"));
        assert!(with_two.contains("TV 10_0_0_6"));
        assert_eq!(parse_groups(&with_two).len(), 2);
    }

    #[test]
    fn remove_assignment_deletes_rule() {
        let with_one = apply_assignments(
            BASE_YAML,
            &[Assignment { ip: "10.0.0.5".into(), name: "Phone".into(), server: Some("X".into()) }],
            &[],
        )
        .unwrap();
        let removed = apply_assignments(
            &with_one,
            &[Assignment { ip: "10.0.0.5".into(), name: "Phone".into(), server: None }],
            &[],
        )
        .unwrap();
        assert!(parse_groups(&removed).is_empty());
        assert!(!removed.contains(GROUPS_BEGIN));
        assert!(removed.contains("GEOIP,RU,DIRECT"));
    }

    #[test]
    fn default_means_remove() {
        let with_one = apply_assignments(
            BASE_YAML,
            &[Assignment { ip: "10.0.0.5".into(), name: "Phone".into(), server: Some("X".into()) }],
            &[],
        )
        .unwrap();
        let removed = apply_assignments(
            &with_one,
            &[Assignment { ip: "10.0.0.5".into(), name: "Phone".into(), server: Some("default".into()) }],
            &[],
        )
        .unwrap();
        assert!(parse_groups(&removed).is_empty());
    }

    #[test]
    fn group_yaml_without_providers_has_no_use() {
        let out = group_yaml("DEV_1_2_3_4", &[]);
        assert!(!out.contains("use:"));
        assert!(out.contains("- Fastest"));
        let out2 = group_yaml("X 1_2_3_4", &["prov1".to_string(), "prov2".to_string()]);
        assert!(out2.contains("use:"));
        assert!(out2.contains("- prov1") && out2.contains("- prov2"));
    }

    #[test]
    fn parse_provider_names_finds_top_level() {
        let yaml = "port: 7890\nproxy-providers:\n  alpha:\n    type: http\n    url: x\n  beta:\n    type: file\nrules:\n  - MATCH,PROXY\n";
        assert_eq!(parse_provider_names(yaml), vec!["alpha".to_string(), "beta".to_string()]);
    }

    #[test]
    fn sanitize_domains_normalizes() {
        let out = sanitize_domains(&[" https://WWW.Example.com/path ".to_string(), "example.com".to_string(), "notadomain".to_string()]);
        assert_eq!(out, vec!["example.com".to_string()]);
    }

    #[test]
    fn domain_rules_inserted_and_removed() {
        let out = apply_domain_rules(BASE_YAML, &["example.com".to_string()], &["forced.org".to_string()]).unwrap();
        assert!(out.contains(DIRECT_BEGIN));
        assert!(out.contains("DOMAIN-SUFFIX,example.com,DIRECT"));
        assert!(out.contains("DOMAIN-SUFFIX,forced.org,PROXY"));
        // доменные правила — сразу после rules: (приоритет над остальными)
        let rpos = out.find("rules:").unwrap();
        let dpos = out.find("DOMAIN-SUFFIX,example.com").unwrap();
        assert!(dpos > rpos && dpos - rpos < 60);
        // повторное применение — без дублей
        let out2 = apply_domain_rules(&out, &["example.com".to_string()], &["forced.org".to_string()]).unwrap();
        assert_eq!(out2.matches("DOMAIN-SUFFIX,example.com").count(), 1);
        // очистка — блоки удалены
        let cleared = apply_domain_rules(&out2, &[], &[]).unwrap();
        assert!(!cleared.contains("DOMAIN-SUFFIX,example.com"));
        assert!(cleared.contains("GEOIP,RU,DIRECT"));
    }

    #[test]
    fn sanitize_name_in_group() {
        assert_eq!(group_name_for("1.2.3.4", "Po'ket,#PC\n"), "PoketPC 1_2_3_4");
        assert_eq!(group_name_for("1.2.3.4", ""), "DEV_1_2_3_4");
    }

    
    const GROUPS_YAML: &str = "proxies:\n  - name: '🇫🇮 Финляндия [⚡ Стабильный ]'\n    type: vless\n  - name: '🇩🇪 Германия'\n    type: vless\nproxy-groups:\n  - name: Fallback\n    type: fallback\n    include-all: true\n    use:\n      - geodema\n\n  - name: Fastest\n    type: url-test\n    include-all: true\n    use:\n      - geodema\n\n  - name: PROXY\n    type: select\n";

    #[test]
    fn ignore_replaces_include_all_with_explicit_list() {
        let out = apply_ignore_to_groups(GROUPS_YAML, &["Финляндия".to_string()]).unwrap();
        assert_eq!(out.matches("    proxies:").count(), 2, "OUT={out}");
        assert!(!out.contains("include-all"));
        assert!(out.contains("- '🇩🇪 Германия'"));
        assert!(!out.contains("- '🇫🇮 Финляндия"));
        assert_eq!(out.matches("exclude-filter: 'Финляндия'").count(), 2);
        let p = out.find("- name: PROXY").unwrap();
        assert!(!out[p..].contains("proxies:"));
    }

    #[test]
    fn ignore_replaces_previous_explicit_list() {
        let with_old = apply_ignore_to_groups(GROUPS_YAML, &["Германия".to_string()]).unwrap();
        let out = apply_ignore_to_groups(&with_old, &["Финляндия".to_string()]).unwrap();
        assert!(out.contains("- '🇩🇪 Германия'"), "OUT={out}");
        assert!(!out.contains("- '🇫🇮 Финляндия"));
    }

    #[test]
    fn ignore_empty_restores_include_all() {
        let with_old = apply_ignore_to_groups(GROUPS_YAML, &["Финляндия".to_string()]).unwrap();
        let out = apply_ignore_to_groups(&with_old, &[]).unwrap();
        assert_eq!(out.matches("include-all: true").count(), 2, "OUT={out}");
        assert!(!out.contains("exclude-filter"), "OUT={out}");
        let g = &out[out.find("proxy-groups:").unwrap()..];
        assert!(!g.contains("    proxies:"), "OUT={out}");
    }

    #[test]
    fn ignore_all_static_drops_proxies_list()
    {
        let out = apply_ignore_to_groups(GROUPS_YAML, &["Финляндия".to_string(), "Германия".to_string()]).unwrap();
        assert!(!out.contains("    proxies:"), "OUT={out}");
        assert!(!out.contains("include-all"));
        assert_eq!(out.matches("exclude-filter").count(), 2);
    }

    #[test]
    fn provider_filter_append_and_restore()
    {
        let pyaml = "proxy-providers:
  geodema:
    type: http
    exclude-filter: \"(?i)DIRECT|Russia|RU\"
    interval: 43200
  geodema2:
    type: http
proxy-groups:
  - name: PROXY
    type: select
";
        let mut saved = std::collections::BTreeMap::new();
        let out = apply_ignore_to_providers(pyaml, &["Германия".to_string()], &mut saved);
        assert!(out.contains("exclude-filter: \"(?i)DIRECT|Russia|RU|Германия\""), "OUT={out}");
        assert_eq!(saved.len(), 1);
        let restored = apply_ignore_to_providers(&out, &[], &mut saved);
        assert!(restored.contains("exclude-filter: \"(?i)DIRECT|Russia|RU\""), "OUT={restored}");
    }

    #[test]
    fn regex_escape_specials() {
        assert_eq!(regex_escape("a.b[c](d)"), "a\\.b\\[c\\]\\(d\\)");
        assert_eq!(regex_escape("plain name"), "plain name");
    }

    #[test]
    fn parse_static_names_skips_groups() {
        let names = parse_static_proxy_names(GROUPS_YAML);
        assert_eq!(names.len(), 2);
        assert!(names[0].starts_with("🇫🇮"));
    }

    #[test]
    fn ip_key_from_group_rejects_non_device_names() {
        assert_eq!(ip_key_from_group("Big PC 192_168_2_118").as_deref(), Some("192.168.2.118"));
        assert_eq!(ip_key_from_group("DEV_aa_bb_cc_dd_ee_ff").as_deref(), Some("aa:bb:cc:dd:ee:ff"));
        assert_eq!(ip_key_from_group("Fallback"), None);
        assert_eq!(ip_key_from_group("NoIpHere"), None);
    }

    #[test]
    fn auto_blocks_inserted_with_newline_when_missing() {
        // YAML без перевода строки после "proxy-groups:" и "rules:" —
        // вставка не должна склеивать маркер с ключом секции.
        let yaml = "proxies:\n  - name: 'srv'\n    type: vless\nproxy-groups:\n  - name: PROXY\n    type: select\nrules:\n  - MATCH,PROXY";
        let a = Assignment {
            ip: "192.168.2.50".into(),
            name: "Тест".into(),
            server: Some("srv".into()),
        };
        let out = apply_assignments(yaml, &[a], &[]).unwrap();
        assert!(out.contains("\nproxy-groups:# --- AUTO-DEVICE-GROUPS-BEGIN ---") == false);
        assert!(out.contains("proxy-groups:\n# --- AUTO-DEVICE-GROUPS-BEGIN ---"));
        assert!(out.contains("rules:\n# --- AUTO-DEVICE-RULES-BEGIN ---"));
        // YAML остаётся валидным по структуре: маркеры на отдельных строках
        for line in out.lines() {
            assert!(!line.starts_with("proxy-groups:#") && !line.starts_with("rules:#"));
        }
    }
}
