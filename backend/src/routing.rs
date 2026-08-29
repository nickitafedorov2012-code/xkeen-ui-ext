//! Раздельная маршрутизация per-device: AUTO-DEVICE-GROUPS / AUTO-DEVICE-RULES блоки
//! в /opt/etc/mihomo/config.yaml. Формат блоков 1:1 с десктопным KeeneticPolicyManager
//! (общее хранилище правил ПК/Android/веб-версий).

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
    /// None или "default" — снять назначение устройства.
    pub server: Option<String>,
}

/// 'Big PC 192_168_2_118' → '192.168.2.118'; 'DEV_aa_bb_cc_dd_ee_ff' → MAC-вид.
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

/// YAML-текст select-группы устройства (формат десктопа).
pub fn group_yaml(group_name: &str) -> String {
    format!(
        "  - name: '{group_name}'\n    type: select\n    proxies:\n      - Fastest\n      - Fallback\n    use:\n      - geodema\n      - geodema2"
    )
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

/// Парсинг существующих правил: ip → исходная строка правила.
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
            .ok_or("В config.yaml нет секции proxy-groups:")?
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
        let pos = content.find("rules:").ok_or("В config.yaml нет секции rules:")? + "rules:".len();
        content = format!("{}{}\n{}", &content[..pos], block, &content[pos..]);
    }

    Ok(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE_YAML: &str = "port: 7890\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies:\n      - Fastest\nrules:\n  - GEOIP,RU,DIRECT\n  - MATCH,PROXY\n";

    #[test]
    fn add_assignment_creates_blocks() {
        let out = apply_assignments(
            BASE_YAML,
            &[Assignment { ip: "192.168.2.118".into(), name: "Big PC".into(), server: Some("🇩🇪 DE".into()) }],
        )
        .unwrap();
        assert!(out.contains(GROUPS_BEGIN));
        assert!(out.contains("- name: 'Big PC 192_168_2_118'"));
        assert!(out.contains("SRC-IP-CIDR,192.168.2.118/32,Big PC 192_168_2_118"));
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
        )
        .unwrap();
        // добавляем второе устройство — первое должно сохраниться
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
}

