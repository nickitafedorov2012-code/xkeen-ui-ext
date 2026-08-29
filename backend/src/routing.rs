//! Р Р°Р·РґРµР»СЊРЅР°СЏ РјР°СЂС€СЂСѓС‚РёР·Р°С†РёСЏ per-device: AUTO-DEVICE-GROUPS / AUTO-DEVICE-RULES Р±Р»РѕРєРё
//! РІ /opt/etc/mihomo/config.yaml. Р¤РѕСЂРјР°С‚ Р±Р»РѕРєРѕРІ 1:1 СЃ РґРµСЃРєС‚РѕРїРЅС‹Рј KeeneticPolicyManager
//! (РѕР±С‰РµРµ С…СЂР°РЅРёР»РёС‰Рµ РїСЂР°РІРёР» РџРљ/Android/РІРµР±-РІРµСЂСЃРёР№).

use regex_lite::Regex;
use std::collections::BTreeMap;

pub const GROUPS_BEGIN: &str = "# --- AUTO-DEVICE-GROUPS-BEGIN ---";
pub const GROUPS_END: &str = "# --- AUTO-DEVICE-GROUPS-END ---";
pub const RULES_BEGIN: &str = "# --- AUTO-DEVICE-RULES-BEGIN ---";
pub const RULES_END: &str = "# --- AUTO-DEVICE-RULES-END ---";

#[derive(Clone, Debug)]
pub struct Assignment {
    pub ip: String,
    pub name: String,
    /// None РёР»Рё "default" вЂ” СЃРЅСЏС‚СЊ РЅР°Р·РЅР°С‡РµРЅРёРµ СѓСЃС‚СЂРѕР№СЃС‚РІР°.
    pub server: Option<String>,
}

/// 'Big PC 192_168_2_118' в†’ '192.168.2.118'; 'DEV_aa_bb_cc_dd_ee_ff' в†’ MAC-РІРёРґ.
pub fn ip_key_from_group(gname: &str) -> String {
    let mut token = gname.trim().split(' ').next_back().unwrap_or("");
    if let Some(stripped) = token.strip_prefix("DEV_") {
        token = stripped;
    }
    let cand = token.replace('_', ".");
    let parts: Vec<&str> = cand.split('.').collect();
    if parts.len() == 4 && parts.iter().all(|p| p.parse::<u8>().is_ok()) {
        return cand;
    }
    if token.contains('_') {
        token.replace('_', ":")
    } else {
        token.to_string()
    }
}

/// РРјСЏ РіСЂСѓРїРїС‹ СѓСЃС‚СЂРѕР№СЃС‚РІР°: '{С‡РёСЃС‚РѕРµ РёРјСЏ} {ip_СЃ_РїРѕРґС‡С‘СЂРєРёРІР°РЅРёСЏРјРё}' РёР»Рё 'DEV_{ip}'.
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

/// YAML-С‚РµРєСЃС‚ select-РіСЂСѓРїРїС‹ СѓСЃС‚СЂРѕР№СЃС‚РІР° (С„РѕСЂРјР°С‚ РґРµСЃРєС‚РѕРїР°).
pub fn group_yaml(group_name: &str) -> String {
    format!(
        "  - name: '{group_name}'\n    type: select\n    proxies:\n      - Fastest\n      - Fallback\n    use:\n      - geodema\n      - geodema2"
    )
}

/// РЎС‚СЂРѕРєР° РїСЂР°РІРёР»Р° РґР»СЏ СѓСЃС‚СЂРѕР№СЃС‚РІР°.
pub fn rule_line(ip: &str, group_name: &str) -> String {
    format!("  - SRC-IP-CIDR,{ip}/32,{group_name}")
}

fn extract_block<'a>(yaml: &'a str, begin: &str, end: &str) -> Option<&'a str> {
    let p1 = yaml.find(begin)? + begin.len();
    let p2 = yaml[p1..].find(end)? + p1;
    Some(&yaml[p1..p2])
}

/// РџР°СЂСЃРёРЅРі СЃСѓС‰РµСЃС‚РІСѓСЋС‰РёС… РіСЂСѓРїРї: ip в†’ РёСЃС…РѕРґРЅС‹Р№ YAML-С‚РµРєСЃС‚ РіСЂСѓРїРїС‹.
pub fn parse_groups(yaml: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Some(block) = extract_block(yaml, GROUPS_BEGIN, GROUPS_END) else {
        return out;
    };
    let re = Regex::new(r"\s{2}- name: '(.+?)'\s*$").unwrap();
    let mut cur_name: Option<String> = None;
    let mut cur_lines: Vec<&str> = Vec::new();
    for line in block.split('\n') {
        match re.captures(line) {
            Some(c) => {
                if let Some(n) = cur_name.take() {
                    out.insert(ip_key_from_group(&n), cur_lines.join("\n"));
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
        out.insert(ip_key_from_group(&n), cur_lines.join("\n"));
    }
    out
}

/// РџР°СЂСЃРёРЅРі СЃСѓС‰РµСЃС‚РІСѓСЋС‰РёС… РїСЂР°РІРёР»: ip в†’ РёСЃС…РѕРґРЅР°СЏ СЃС‚СЂРѕРєР° РїСЂР°РІРёР»Р°.
pub fn parse_rules(yaml: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Some(block) = extract_block(yaml, RULES_BEGIN, RULES_END) else {
        return out;
    };
    let re = Regex::new(r"\s*- SRC-IP-CIDR,(.+?)/32,").unwrap();
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

/// РЈРґР°Р»РµРЅРёРµ AUTO-Р±Р»РѕРєРѕРІ РёР· YAML (РґР»СЏ РїРѕСЃР»РµРґСѓСЋС‰РµР№ РІСЃС‚Р°РІРєРё РѕР±СЉРµРґРёРЅС‘РЅРЅС‹С…).
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

/// Merge-СЃРµРјР°РЅС‚РёРєР° РґРµСЃРєС‚РѕРїР°: СЃСѓС‰РµСЃС‚РІСѓСЋС‰РёРµ РїСЂР°РІРёР»Р° СЃРѕС…СЂР°РЅСЏСЋС‚СЃСЏ, РїСЂРёРјРµРЅСЏСЋС‚СЃСЏ С‚РѕР»СЊРєРѕ
/// РїРµСЂРµРґР°РЅРЅС‹Рµ РЅР°Р·РЅР°С‡РµРЅРёСЏ (server=None/"default" вЂ” СЃРЅСЏС‚СЊ). Р’РѕР·РІСЂР°С‰Р°РµС‚ РЅРѕРІС‹Р№ YAML.
pub fn apply_assignments(yaml: &str, assignments: &[Assignment]) -> Result<String, String> {
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
        groups_by_ip.insert(ip.to_string(), group_yaml(&gname));
        rules_by_ip.insert(ip.to_string(), rule_line(ip, &gname));
        let _ = server;
    }

    let mut content = remove_blocks(yaml);

    let groups_sorted: Vec<String> = groups_by_ip.values().cloned().collect();
    if !groups_sorted.is_empty() {
        let block = format!("{GROUPS_BEGIN}\n{}\n{GROUPS_END}\n", groups_sorted.join("\n"));
        let pos = content
            .find("proxy-groups:")
            .ok_or("Р’ config.yaml РЅРµС‚ СЃРµРєС†РёРё proxy-groups:")?
            + "proxy-groups:".len();
        content = format!("{}{}\n{}", &content[..pos], block, &content[pos..]);
    }

    let rules_sorted: Vec<String> = rules_by_ip
        .iter()
        .filter(|(ip, _)| groups_by_ip.contains_key(*ip))
        .map(|(_, line)| line.clone())
        .collect();
    if !rules_sorted.is_empty() {
        let block = format!("{RULES_BEGIN}\n{}\n{RULES_END}\n", rules_sorted.join("\n"));
        let pos = content.find("rules:").ok_or("Р’ config.yaml РЅРµС‚ СЃРµРєС†РёРё rules:")? + "rules:".len();
        content = format!("{}{}\n{}", &content[..pos], block, &content[pos..]);
    }

    Ok(content)
}

/// Р­РєСЂР°РЅРёСЂРѕРІР°РЅРёРµ РёРјРµРЅРё СЃРµСЂРІРµСЂР° РґР»СЏ regex (exclude-filter РёСЃРїРѕР»СЊР·СѓРµС‚ Go regexp).
/// Экранирование имени сервера для regex (exclude-filter провайдеров).
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
        )
        .unwrap();
        assert!(out.contains(GROUPS_BEGIN));
        assert!(out.contains("- name: 'Big PC 192_168_2_118'"));
        assert!(out.contains("SRC-IP-CIDR,192.168.2.118/32,Big PC 192_168_2_118"));
        // Р±Р»РѕРєРё РІСЃС‚Р°РІР»РµРЅС‹ СЃСЂР°Р·Сѓ РїРѕСЃР»Рµ СЃРµРєС†РёР№
        let gpos = out.find("proxy-groups:").unwrap();
        let bpos = out.find(GROUPS_BEGIN).unwrap();
        assert!(bpos > gpos && bpos - gpos < 20);
        let rpos = out.find("rules:").unwrap();
        let rbpos = out.find(RULES_BEGIN).unwrap();
        assert!(rbpos > rpos && rbpos - rpos < 20);
        // РёСЃС…РѕРґРЅС‹Рµ РїСЂР°РІРёР»Р° РЅРµ С‚СЂРѕРЅСѓС‚С‹
        assert!(out.contains("GEOIP,RU,DIRECT"));
    }

    #[test]
    fn merge_preserves_other_devices() {
        let with_one = apply_assignments(
            BASE_YAML,
            &[Assignment { ip: "10.0.0.5".into(), name: "Phone".into(), server: Some("X".into()) }],
        )
        .unwrap();
        // РґРѕР±Р°РІР»СЏРµРј РІС‚РѕСЂРѕРµ СѓСЃС‚СЂРѕР№СЃС‚РІРѕ вЂ” РїРµСЂРІРѕРµ РґРѕР»Р¶РЅРѕ СЃРѕС…СЂР°РЅРёС‚СЊСЃСЏ
        let with_two = apply_assignments(
            &with_one,
            &[Assignment { ip: "10.0.0.6".into(), name: "TV".into(), server: Some("Y".into()) }],
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
        )
        .unwrap();
        let removed = apply_assignments(
            &with_one,
            &[Assignment { ip: "10.0.0.5".into(), name: "Phone".into(), server: None }],
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
        )
        .unwrap();
        let removed = apply_assignments(
            &with_one,
            &[Assignment { ip: "10.0.0.5".into(), name: "Phone".into(), server: Some("default".into()) }],
        )
        .unwrap();
        assert!(parse_groups(&removed).is_empty());
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
}
