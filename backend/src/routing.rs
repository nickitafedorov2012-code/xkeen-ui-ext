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
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*- SRC-IP-CIDR,([0-9a-fA-F\.:]+)(?:/([0-9]{1,3}))?,").unwrap());
    &RE
}

pub const GROUPS_BEGIN: &str = "# --- AUTO-DEVICE-GROUPS-BEGIN ---";
pub const GROUPS_END: &str = "# --- AUTO-DEVICE-GROUPS-END ---";
pub const RULES_BEGIN: &str = "# --- AUTO-DEVICE-RULES-BEGIN ---";
pub const RULES_END: &str = "# --- AUTO-DEVICE-RULES-END ---";

pub use crate::whitelist_catalog::*;

pub const WHITELIST_BEGIN: &str = "# --- AUTO-WHITELIST-BEGIN ---";
pub const WHITELIST_END: &str = "# --- AUTO-WHITELIST-END ---";
pub const WHITELIST_DNS_BEGIN: &str = "# --- AUTO-WHITELIST-DNS-BEGIN ---";
pub const WHITELIST_DNS_END: &str = "# --- AUTO-WHITELIST-DNS-END ---";
pub const WHITELIST_GROUP_BEGIN: &str = "# --- AUTO-WHITELIST-GROUP-BEGIN ---";
pub const WHITELIST_GROUP_END: &str = "# --- AUTO-WHITELIST-GROUP-END ---";
pub const WHITELIST_GROUP_NAME: &str = "🛡️ Whitelist";


pub fn validate_marker_pair(yaml: &str, begin: &str, end: &str) -> Result<(), String> {
    let mut opened = false;
    let mut seen = false;
    for (index, line) in yaml.lines().enumerate() {
        let t = line.trim();
        if t == begin {
            if opened || seen {
                return Err(format!("Повторный или вложенный маркер {}, строка {}", begin, index + 1));
            }
            opened = true;
            seen = true;
        } else if t == end {
            if !opened {
                return Err(format!("Маркер {} без соответствующего BEGIN, строка {}", end, index + 1));
            }
            opened = false;
        }
    }
    if opened {
        return Err(format!("Отсутствует завершающий маркер {}", end));
    }
    Ok(())
}

pub fn section_end_offset(yaml: &str, section: &str) -> Result<usize, String> {
    let mut offset = 0;
    let mut found = None;
    for line in yaml.split_inclusive('\n') {
        if line.trim_end() == section {
            if found.is_some() { return Err(format!("Повторная секция {}", section)); }
            found = Some(offset + section.len());
        }
        offset += line.len();
    }
    found.ok_or_else(|| format!("В конфиге нет секции {}", section))
}

pub const PROVIDER_DEFAULT_INTERVAL_SECS: u32 = 86400;
pub const PROVIDER_HEALTH_CHECK_URL: &str = "https://www.gstatic.com/generate_204";
pub const PROVIDER_HEALTH_CHECK_INTERVAL_SECS: u32 = 300;
pub const PROVIDER_HEALTH_CHECK_EXPECTED_STATUS: u16 = 204;

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
    let parts: Vec<&str> = token.split('_').collect();
    if parts.len() == 5 && parts[..4].iter().all(|p| p.parse::<u8>().is_ok()) && parts[4].parse::<u8>().is_ok() {
        let ip_cand = parts[..4].join(".");
        let prefix = parts[4];
        return Some(format!("{ip_cand}/{prefix}"));
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
    let safe_ip = ip.replace(['.', ':', '/'], "_");
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
        "  - name: '{group_name}'\n    type: select\n    proxies:\n      - Fastest\n      - Fallback\n      - DIRECT\n      - REJECT"
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

/// Карта id провайдера -> url из блока proxy-providers в YAML
pub fn parse_provider_urls(yaml: &str) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    let mut in_providers = false;
    let mut cur_provider: Option<String> = None;

    for line in yaml.lines() {
        let trimmed = line.trim();
        let is_top = !line.starts_with(' ') && !trimmed.is_empty();

        if line.trim_end() == "proxy-providers:" {
            in_providers = true;
            cur_provider = None;
            continue;
        }
        if in_providers && is_top {
            break;
        }
        if in_providers {
            if line.starts_with("  ") && !line.starts_with("   ") && trimmed.ends_with(':') && !trimmed.starts_with('-') {
                cur_provider = Some(trimmed.trim_end_matches(':').to_string());
                continue;
            }
            if line.starts_with("    url:") || (trimmed.starts_with("url:") && !line.starts_with("      ")) {
                if let Some(ref p) = cur_provider {
                    if !out.contains_key(p) {
                        let val = trimmed.trim_start_matches("url:").trim().trim_matches('"').trim_matches('\'').to_string();
                        if !val.is_empty() {
                            out.insert(p.clone(), val);
                        }
                    }
                }
            }
        }
    }
    out
}

/// Карта id провайдера -> x-hwid из блока proxy-providers в YAML
pub fn parse_provider_hwids(yaml: &str) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    let mut in_providers = false;
    let mut cur_provider: Option<String> = None;

    for line in yaml.lines() {
        let trimmed = line.trim();
        let is_top = !line.starts_with(' ') && !trimmed.is_empty();

        if line.trim_end() == "proxy-providers:" {
            in_providers = true;
            cur_provider = None;
            continue;
        }
        if in_providers && is_top {
            break;
        }
        if in_providers {
            if line.starts_with("  ") && !line.starts_with("   ") && trimmed.ends_with(':') && !trimmed.starts_with('-') {
                cur_provider = Some(trimmed.trim_end_matches(':').to_string());
                continue;
            }
            if trimmed.starts_with("x-hwid:") {
                if let Some(ref p) = cur_provider {
                    let val = trimmed
                        .trim_start_matches("x-hwid:")
                        .trim()
                        .trim_matches(|c| c == '[' || c == ']' || c == '"' || c == '\'' || c == ' ')
                        .to_string();
                    if !val.is_empty() {
                        out.insert(p.clone(), val);
                    }
                }
            }
        }
    }
    out
}

/// Добавить новую HTTP-подписку в блок proxy-providers в config.yaml с поддержкой HWID и User-Agent
pub fn add_provider_to_yaml_full(
    yaml: &str,
    id: &str,
    url: &str,
    health_check_url: Option<&str>,
    health_check_interval: Option<u32>,
    hwid: Option<&str>,
    user_agent: Option<&str>,
) -> Result<String, String> {
    let id = id.trim();
    let url = url.trim();
    if id.is_empty() || url.is_empty() {
        return Err("ID подписки и URL не могут быть пустыми".into());
    }

    let forbidden_chars = ['\r', '\n', '"', '\'', ','];
    if [id, url].iter().any(|s| s.chars().any(|c| forbidden_chars.contains(&c)))
        || hwid.map(|h| h.chars().any(|c| forbidden_chars.contains(&c))).unwrap_or(false)
        || user_agent.map(|u| u.chars().any(|c| forbidden_chars.contains(&c))).unwrap_or(false)
    {
        return Err("Параметры подписки содержат недопустимые символы (переводы строк, кавычки или запятые)".into());
    }

    let existing = parse_provider_names(yaml);
    if existing.iter().any(|p| p.eq_ignore_ascii_case(id)) {
        return Err(format!("Подписка с ID '{id}' уже существует"));
    }

    let hc_url = health_check_url.unwrap_or(PROVIDER_HEALTH_CHECK_URL);
    let hc_interval = health_check_interval.unwrap_or(PROVIDER_HEALTH_CHECK_INTERVAL_SECS);

    let ua = user_agent.unwrap_or("ClashMeta/1.19.24; mihomo/1.19.24");
    let mut header_lines = format!("    header:\n      User-Agent: [\"{ua}\"]");
    if let Some(h) = hwid.map(str::trim).filter(|h| !h.is_empty()) {
        header_lines.push_str(&format!("\n      x-hwid: [\"{h}\"]"));
    }

    let is_crlf = yaml.contains("\r\n");
    let eol = if is_crlf { "\r\n" } else { "\n" };

    let new_block = format!(
        "  {id}:\n    type: http\n    url: \"{url}\"\n    interval: {PROVIDER_DEFAULT_INTERVAL_SECS}\n    health-check:\n      enable: true\n      lazy: true\n      url: \"{hc_url}\"\n      interval: {hc_interval}\n      expected-status: {PROVIDER_HEALTH_CHECK_EXPECTED_STATUS}\n{header_lines}\n    override:\n      udp: true\n      tfo: true"
    );
    let new_block_formatted = if is_crlf {
        new_block.replace('\n', "\r\n")
    } else {
        new_block
    };

    let mut out = Vec::new();
    let mut in_providers = false;
    let mut inserted = false;

    for line in yaml.lines() {
        let trimmed = line.trim();
        let is_top = !line.starts_with(' ') && !trimmed.is_empty();

        if line.trim_end() == "proxy-providers:" {
            in_providers = true;
            out.push(line.to_string());
            out.push(new_block_formatted.clone());
            inserted = true;
            continue;
        }
        if in_providers && is_top {
            in_providers = false;
        }
        out.push(line.to_string());
    }

    if !inserted {
        out.push(format!("{eol}proxy-providers:"));
        out.push(new_block_formatted);
    }

    Ok(out.join(eol))
}

/// Добавить новую HTTP-подписку в блок proxy-providers в config.yaml
pub fn add_provider_to_yaml(
    yaml: &str,
    id: &str,
    url: &str,
    health_check_url: Option<&str>,
    health_check_interval: Option<u32>,
) -> Result<String, String> {
    add_provider_to_yaml_full(yaml, id, url, health_check_url, health_check_interval, None, None)
}

/// Добавить локальную file-подписку (для V2Ray / Base64 конвертированных списков) в config.yaml
pub fn add_file_provider_to_yaml(
    yaml: &str,
    id: &str,
    file_path: &str,
    health_check_url: Option<&str>,
    health_check_interval: Option<u32>,
) -> Result<String, String> {
    let id = id.trim();
    let file_path = file_path.trim();
    if id.is_empty() || file_path.is_empty() {
        return Err("ID подписки и путь к файлу не могут быть пустыми".into());
    }

    let existing = parse_provider_names(yaml);
    if existing.iter().any(|p| p.eq_ignore_ascii_case(id)) {
        return Err(format!("Подписка с ID '{id}' уже существует"));
    }

    let hc_url = health_check_url.unwrap_or(PROVIDER_HEALTH_CHECK_URL);
    let hc_interval = health_check_interval.unwrap_or(PROVIDER_HEALTH_CHECK_INTERVAL_SECS);

    let is_crlf = yaml.contains("\r\n");
    let eol = if is_crlf { "\r\n" } else { "\n" };

    let new_block = format!(
        "  {id}:\n    type: file\n    path: \"{file_path}\"\n    health-check:\n      enable: true\n      lazy: true\n      url: \"{hc_url}\"\n      interval: {hc_interval}\n      expected-status: {PROVIDER_HEALTH_CHECK_EXPECTED_STATUS}\n    override:\n      udp: true\n      tfo: true"
    );
    let new_block_formatted = if is_crlf {
        new_block.replace('\n', "\r\n")
    } else {
        new_block
    };

    let mut out = Vec::new();
    let mut in_providers = false;
    let mut inserted = false;

    for line in yaml.lines() {
        let trimmed = line.trim();
        let is_top = !line.starts_with(' ') && !trimmed.is_empty();

        if line.trim_end() == "proxy-providers:" {
            in_providers = true;
            out.push(line.to_string());
            out.push(new_block_formatted.clone());
            inserted = true;
            continue;
        }
        if in_providers && is_top {
            in_providers = false;
        }
        out.push(line.to_string());
    }

    if !inserted {
        out.push(format!("{eol}proxy-providers:"));
        out.push(new_block_formatted);
    }

    Ok(out.join(eol))
}

/// Преобразовать существующий HTTP-провайдер в локальный file-провайдер (type: file).
/// Используется для предотвращения запросов ядра Mihomo с User-Agent: ClashMeta к провайдерам,
/// которые отдают урезанные/технические имена серверов ObsV-.
pub fn convert_http_provider_to_file_in_yaml(yaml: &str, id: &str, file_path: &str) -> Result<String, String> {
    let id_trimmed = id.trim();
    if id_trimmed.is_empty() {
        return Err("ID провайдера не может быть пустым".into());
    }
    let eol = if yaml.contains("\r\n") { "\r\n" } else { "\n" };
    let mut out = Vec::new();
    let mut in_providers = false;
    let mut in_target = false;
    let mut target_found = false;
    let mut path_written = false;

    for line in yaml.lines() {
        let trimmed = line.trim();
        let is_top = !line.starts_with(' ') && !trimmed.is_empty();

        if line.trim_end() == "proxy-providers:" {
            in_providers = true;
            in_target = false;
            out.push(line.to_string());
            continue;
        }
        if in_providers && is_top {
            in_providers = false;
            in_target = false;
            out.push(line.to_string());
            continue;
        }
        if in_providers {
            if line.starts_with("  ") && !line.starts_with("   ") && trimmed.ends_with(':') && !trimmed.starts_with('-') {
                let prov_name = trimmed.trim_end_matches(':');
                if prov_name.eq_ignore_ascii_case(id_trimmed) {
                    in_target = true;
                    target_found = true;
                    path_written = false;
                    out.push(line.to_string());
                    continue;
                } else {
                    in_target = false;
                }
            }

            if in_target {
                if line.starts_with("    type:") {
                    out.push("    type: file".to_string());
                    if !path_written {
                        out.push(format!("    path: \"{file_path}\""));
                        path_written = true;
                    }
                    continue;
                }
                if line.starts_with("    path:") {
                    if !path_written {
                        out.push(format!("    path: \"{file_path}\""));
                        path_written = true;
                    }
                    continue;
                }
                // Пропускаем url, interval и exclude-filter на верхнем уровне провайдера (4 пробела)
                if line.starts_with("    url:")
                    || line.starts_with("    interval:")
                    || line.starts_with("    exclude-filter:")
                {
                    continue;
                }
                out.push(line.to_string());
                continue;
            }
        }
        out.push(line.to_string());
    }

    if !target_found {
        return Err(format!("Провайдер '{id_trimmed}' не найден в config.yaml"));
    }
    Ok(out.join(eol))
}

/// Удалить подписку из блока proxy-providers и всех proxy-groups (секция use:) в config.yaml
pub fn delete_provider_from_yaml(yaml: &str, id: &str) -> Result<String, String> {
    let id = id.trim();
    if id.is_empty() {
        return Err("ID подписки не может быть пустым".into());
    }

    let mut out = Vec::new();
    let mut in_providers = false;
    let mut skipping_target = false;
    let mut in_use_block = false;
    let mut use_indent = 0;

    for line in yaml.lines() {
        let trimmed = line.trim();
        let is_top = !line.starts_with(' ') && !trimmed.is_empty();

        // Отслеживаем вход в proxy-providers
        if line.trim_end() == "proxy-providers:" {
            in_providers = true;
            skipping_target = false;
            in_use_block = false;
            out.push(line.to_string());
            continue;
        }
        if in_providers && is_top {
            in_providers = false;
            skipping_target = false;
        }

        if in_providers {
            if line.starts_with("  ") && !line.starts_with("   ") && trimmed.ends_with(':') && !trimmed.starts_with('-') {
                let pname = trimmed.trim_end_matches(':');
                if pname.eq_ignore_ascii_case(id) {
                    skipping_target = true;
                    continue;
                } else {
                    skipping_target = false;
                }
            }
            if skipping_target {
                continue;
            }
            out.push(line.to_string());
            continue;
        }

        // Отслеживаем блок use: внутри proxy-groups
        if trimmed == "use:" || trimmed.starts_with("use: ") {
            let indent = line.len() - line.trim_start().len();
            if trimmed == "use:" {
                in_use_block = true;
                use_indent = indent;
                out.push(line.to_string());
                continue;
            } else {
                // Однострочный inline use: [prov1, prov2, ...]
                let raw_val = trimmed.trim_start_matches("use:").trim();
                if raw_val.starts_with('[') && raw_val.ends_with(']') {
                    let inner = &raw_val[1..raw_val.len() - 1];
                    let remaining: Vec<&str> = inner
                        .split(',')
                        .map(str::trim)
                        .filter(|item| {
                            let clean = item.trim_matches('\'').trim_matches('"').trim();
                            !clean.eq_ignore_ascii_case(id)
                        })
                        .collect();
                    let prefix = &line[..indent];
                    out.push(format!("{prefix}use: [{}]", remaining.join(", ")));
                    continue;
                }
            }
        }

        if in_use_block {
            let indent = line.len() - line.trim_start().len();
            if indent > use_indent && trimmed.starts_with('-') {
                let item = trimmed.trim_start_matches('-').trim();
                let clean = item.trim_matches('\'').trim_matches('"').trim();
                if clean.eq_ignore_ascii_case(id) {
                    // Пропускаем удаляемый провайдер из use:
                    continue;
                }
            } else if !trimmed.is_empty() {
                in_use_block = false;
            }
        }

        out.push(line.to_string());
    }

    let is_crlf = yaml.contains("\r\n");
    let eol = if is_crlf { "\r\n" } else { "\n" };
    Ok(out.join(eol))
}

/// Синхронизация списка отключенных подписок в секциях use: proxy-groups.
/// Для отключенных подписок строки закомментируются (`# - sub_id`),
/// для включенных — раскомментируются обратно (`- sub_id`), не нарушая порядок и отступы.
pub fn sync_disabled_providers_in_yaml(yaml: &str, disabled_providers: &[String]) -> Result<String, String> {
    let mut out = Vec::new();
    let mut in_use_block = false;
    let mut use_indent = 0;

    for line in yaml.lines() {
        let trimmed = line.trim();
        let is_top = !line.starts_with(' ') && !trimmed.is_empty();

        if is_top {
            in_use_block = false;
        }

        // Inline use: [sub1, sub2]
        if (trimmed.starts_with("use: [") || trimmed.starts_with("use:[")) && trimmed.ends_with(']') {
            let raw_val = trimmed.trim_start_matches("use:").trim();
            let inner = &raw_val[1..raw_val.len() - 1];
            let items: Vec<&str> = inner.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
            let remaining: Vec<&str> = items
                .into_iter()
                .filter(|item| {
                    let clean = item.trim_matches('\'').trim_matches('"').trim();
                    !disabled_providers.iter().any(|dp| dp.eq_ignore_ascii_case(clean))
                })
                .collect();
            let indent_spaces = &line[..line.len() - line.trim_start().len()];
            out.push(format!("{indent_spaces}use: [{}]", remaining.join(", ")));
            continue;
        }

        if trimmed == "use:" {
            in_use_block = true;
            use_indent = line.len() - line.trim_start().len();
            out.push(line.to_string());
            continue;
        }

        if in_use_block {
            let indent = line.len() - line.trim_start().len();
            if indent > use_indent {
                if trimmed.starts_with('-') {
                    let prov_id = trimmed.trim_start_matches('-').trim().trim_matches('\'').trim_matches('"');
                    let is_disabled = disabled_providers.iter().any(|dp| dp.eq_ignore_ascii_case(prov_id));
                    if is_disabled {
                        let prefix = &line[..indent];
                        out.push(format!("{prefix}# - {prov_id}"));
                        continue;
                    }
                } else if trimmed.starts_with('#') {
                    let comment_content = trimmed.trim_start_matches('#').trim();
                    if comment_content.starts_with('-') {
                        let prov_id = comment_content.trim_start_matches('-').trim().trim_matches('\'').trim_matches('"');
                        let is_disabled = disabled_providers.iter().any(|dp| dp.eq_ignore_ascii_case(prov_id));
                        if !is_disabled {
                            let prefix = &line[..indent];
                            out.push(format!("{prefix}- {prov_id}"));
                            continue;
                        }
                    }
                }
            } else if !trimmed.is_empty() {
                in_use_block = false;
            }
        }

        out.push(line.to_string());
    }

    let is_crlf = yaml.contains("\r\n");
    let eol = if is_crlf { "\r\n" } else { "\n" };
    Ok(out.join(eol))
}

fn replace_hwid_in_url_line(line: &str, new_hwid: &str) -> String {
    if let Some(pos) = line.find("hwid=") {
        let after = &line[pos + 5..];
        let end_pos = after.find(|c: char| c == '&' || c == '"' || c == '\'' || c == ' ' || c == '\r' || c == '\n').unwrap_or(after.len());
        format!("{}hwid={}{}", &line[..pos], new_hwid, &after[end_pos..])
    } else {
        line.to_string()
    }
}

fn remove_hwid_from_url_line(line: &str) -> String {
    if let Some(pos) = line.find("hwid=") {
        let start_pos = if pos > 0 && (&line[pos - 1..pos] == "&" || &line[pos - 1..pos] == "?") {
            pos - 1
        } else {
            pos
        };
        let after = &line[pos + 5..];
        let end_pos = after.find(|c: char| c == '&' || c == '"' || c == '\'' || c == ' ' || c == '\r' || c == '\n').unwrap_or(after.len());
        let rest = &after[end_pos..];
        let was_question = pos > 0 && &line[pos - 1..pos] == "?";
        if was_question && rest.starts_with('&') {
            format!("{}?{}", &line[..start_pos], &rest[1..])
        } else {
            format!("{}{}", &line[..start_pos], rest)
        }
    } else {
        line.to_string()
    }
}

/// Обновить или удалить HWID подписки в config.yaml (в блоке header и query string URL)
pub fn update_provider_hwid_in_yaml(yaml: &str, provider_id: &str, new_hwid: Option<&str>) -> Result<String, String> {
    let id = provider_id.trim();
    if id.is_empty() {
        return Err("ID подписки не может быть пустым".into());
    }

    let is_crlf = yaml.contains("\r\n");
    let eol = if is_crlf { "\r\n" } else { "\n" };

    let mut out: Vec<String> = Vec::new();
    let mut in_providers = false;
    let mut in_target_provider = false;
    let mut found_provider = false;
    let mut in_header = false;
    let mut header_indent = 4;
    let mut handled_hwid = false;

    let clean_hwid = new_hwid.map(str::trim).filter(|h| !h.is_empty());

    for line in yaml.lines() {
        let trimmed = line.trim();
        let is_top = !line.starts_with(' ') && !trimmed.is_empty();

        if line.trim_end() == "proxy-providers:" {
            in_providers = true;
            out.push(line.to_string());
            continue;
        }

        if in_providers && is_top {
            if in_target_provider && !handled_hwid {
                if let Some(hwid) = clean_hwid {
                    out.push(format!("    header:\n      x-hwid: [\"{hwid}\"]"));
                    handled_hwid = true;
                }
            }
            in_providers = false;
            in_target_provider = false;
            in_header = false;
            out.push(line.to_string());
            continue;
        }

        if in_providers {
            if line.starts_with("  ") && !line.starts_with("   ") && trimmed.ends_with(':') && !trimmed.starts_with('-') {
                let cur = trimmed.trim_end_matches(':').trim();
                if in_target_provider && !handled_hwid {
                    if let Some(hwid) = clean_hwid {
                        out.push(format!("    header:\n      x-hwid: [\"{hwid}\"]"));
                        handled_hwid = true;
                    }
                }
                if cur.eq_ignore_ascii_case(id) {
                    in_target_provider = true;
                    found_provider = true;
                    in_header = false;
                    handled_hwid = false;
                } else {
                    in_target_provider = false;
                    in_header = false;
                }
                out.push(line.to_string());
                continue;
            }

            if in_target_provider {
                if trimmed.starts_with("url:") {
                    let url_line = if let Some(hwid) = clean_hwid {
                        if line.contains("hwid=") {
                            replace_hwid_in_url_line(line, hwid)
                        } else {
                            line.to_string()
                        }
                    } else if line.contains("hwid=") {
                        remove_hwid_from_url_line(line)
                    } else {
                        line.to_string()
                    };
                    out.push(url_line);
                    continue;
                }

                if trimmed == "header:" || trimmed.starts_with("header: ") {
                    in_header = true;
                    header_indent = line.len() - line.trim_start().len();
                    out.push(line.to_string());
                    continue;
                }

                if in_header {
                    let indent = line.len() - line.trim_start().len();
                    if indent > header_indent {
                        if trimmed.starts_with("x-hwid:") {
                            if let Some(hwid) = clean_hwid {
                                let prefix = &line[..indent];
                                out.push(format!("{prefix}x-hwid: [\"{hwid}\"]"));
                                handled_hwid = true;
                            }
                            continue; // Если None - пропускаем строку (удаляем HWID)
                        }
                    } else if !trimmed.is_empty() {
                        in_header = false;
                        if !handled_hwid {
                            if let Some(hwid) = clean_hwid {
                                let prefix = " ".repeat(header_indent + 2);
                                out.push(format!("{prefix}x-hwid: [\"{hwid}\"]"));
                                handled_hwid = true;
                            }
                        }
                    }
                }

                if (trimmed.starts_with("override:") || trimmed.starts_with("interval:")) && !handled_hwid {
                    if let Some(hwid) = clean_hwid {
                        out.push(format!("    header:\n      x-hwid: [\"{hwid}\"]"));
                        handled_hwid = true;
                    }
                }
            }
        }

        out.push(line.to_string());
    }

    if in_target_provider && !handled_hwid {
        if let Some(hwid) = clean_hwid {
            out.push(format!("    header:\n      x-hwid: [\"{hwid}\"]"));
        }
    }

    if !found_provider {
        return Err(format!("Подписка с ID '{id}' не найдена в config.yaml"));
    }

    Ok(out.join(eol))
}

/// Домены: очистка и нормализация (нижний регистр, без портов/протоколов/путей/userinfo).
pub fn sanitize_domains(list: &[String]) -> Vec<String> {
    let mut out: Vec<String> = list
        .iter()
        .map(|d| crate::config::normalize_domain(d))
        .filter(|d| !d.is_empty() && d.contains('.') && !d.contains('/') && !d.contains(' '))
        .collect();
    out.sort();
    out.dedup();
    out
}

// --- Доменные списки: AUTO-DIRECT / AUTO-FORCE / AUTO-DEVICE-DOMAINS блоки в rules ---

pub const DIRECT_BEGIN: &str = "# --- AUTO-DIRECT-BEGIN ---";
pub const DIRECT_END: &str = "# --- AUTO-DIRECT-END ---";
pub const FORCE_BEGIN: &str = "# --- AUTO-FORCE-BEGIN ---";
pub const FORCE_END: &str = "# --- AUTO-FORCE-END ---";
pub const DEV_DOMAINS_BEGIN: &str = "# --- AUTO-DEVICE-DOMAINS-BEGIN ---";
pub const DEV_DOMAINS_END: &str = "# --- AUTO-DEVICE-DOMAINS-END ---";

pub const ADBLOCK_BEGIN: &str = "# --- AUTO-ADBLOCK-RULES-BEGIN ---";
pub const ADBLOCK_END: &str = "# --- AUTO-ADBLOCK-RULES-END ---";
pub const ADBLOCK_RULE: &str = "  - GEOSITE,category-ads-all,REJECT";

pub const AGH_DNS_BEGIN: &str = "# --- AUTO-ADGUARD-DNS-BEGIN ---";
pub const AGH_DNS_END: &str = "# --- AUTO-ADGUARD-DNS-END ---";
pub const AGH_DIRECT_DNS_BEGIN: &str = "# --- AUTO-ADGUARD-DIRECT-DNS-BEGIN ---";
pub const AGH_DIRECT_DNS_END: &str = "# --- AUTO-ADGUARD-DIRECT-DNS-END ---";

pub const GOOGLE_AI_BEGIN: &str = "# --- AUTO-GOOGLE-AI-BEGIN ---";
pub const GOOGLE_AI_END: &str = "# --- AUTO-GOOGLE-AI-END ---";

pub const ZAPRET_HYBRID_BEGIN: &str = "# --- AUTO-ZAPRET-HYBRID-BEGIN ---";
pub const ZAPRET_HYBRID_END: &str = "# --- AUTO-ZAPRET-HYBRID-END ---";

pub const GAMING_BEGIN: &str = "# --- AUTO-GAMING-RULES-BEGIN ---";
pub const GAMING_END: &str = "# --- AUTO-GAMING-RULES-END ---";
pub const GAMING_GROUP_BEGIN: &str = "# --- AUTO-GAMING-GROUP-BEGIN ---";
pub const GAMING_GROUP_END: &str = "# --- AUTO-GAMING-GROUP-END ---";
pub const GAMING_GROUP_NAME: &str = "🎮 Gaming";

pub const GAMING_DISCORD_DOMAINS: &[&str] = &[
    "discord.com", "discordapp.com", "discord.gg", "discordapp.net", "discord.media", "discord.co", "dis.gd",
];

pub const GAMING_STEAM_DOMAINS: &[&str] = &[
    "steamcommunity.com", "steampowered.com", "steamstatic.com", "steamserver.net", "valvesoftware.com", "steamcontent.com",
];

pub const GAMING_PSN_DOMAINS: &[&str] = &[
    "playstation.com", "playstation.net", "sonyentertainmentnetwork.com", "playstationnetwork.com",
];

pub const GAMING_XBOX_DOMAINS: &[&str] = &[
    "xbox.com", "xboxlive.com", "gamepass.com",
    "user.auth.xboxlive.com", "device.auth.xboxlive.com", "title.auth.xboxlive.com", "xsts.auth.xboxlive.com",
];

pub const GAMING_BATTLENET_DOMAINS: &[&str] = &[
    "battle.net", "blizzard.com", "blzstatic.com",
];

pub const GAMING_EPIC_DOMAINS: &[&str] = &[
    "epicgames.com", "unrealengine.com", "epicgames.dev",
];

pub const GAMING_EA_DOMAINS: &[&str] = &[
    "ea.com", "origin.com", "electronicarts.com",
];

pub const GAMING_RIOT_DOMAINS: &[&str] = &[
    "riotgames.com", "leagueoflegends.com", "pvp.net", "riotcdn.net",
];

pub const GAMING_SUPERCELL_DOMAINS: &[&str] = &[
    "supercell.com", "supercellid.com", "brawlstars.com", "brawlstarsgame.com", "clashofclans.com", "clashroyale.com",
];

pub const GAMING_NINTENDO_DOMAINS: &[&str] = &[
    "nintendo.com", "nintendo.net", "nintendo.eu", "nintendo-europe.com",
];

pub const GAMING_ROBLOX_DOMAINS: &[&str] = &[
    "roblox.com", "rbxcdn.com",
];

pub const YOUTUBE_HYBRID_DOMAINS: &[&str] = &[
    "googlevideo.com",
    "youtube.com",
    "ytimg.com",
    "youtu.be",
    "yt.be",
    "ggpht.com",
    "youtube-nocookie.com",
];

pub const SMART_TV_GGC_DOMAINS: &[&str] = &[
    "googlevideo.com",
    "youtube.com",
    "ytimg.com",
    "youtu.be",
    "yt.be",
    "ggpht.com",
    "youtube-nocookie.com",
    "redirector.googlevideo.com",
    "manifest.googlevideo.com",
    "gvt1.com",
    "play.google.com",
];

pub const DISCORD_HYBRID_DOMAINS: &[&str] = &[
    "discord.com",
    "discord.gg",
    "discordapp.com",
    "discordapp.net",
    "discord.media",
    "discord.co",
    "dis.gd",
    "discord-activities.com",
];

pub const GITHUB_DOMAINS: &[&str] = &[
    "github.com",
    "githubassets.com",
    "githubusercontent.com",
    "raw.githubusercontent.com",
    "assets-cdn.github.com",
    "objects.githubusercontent.com",
    "codeload.github.com",
    "avatars.githubusercontent.com",
    "api.github.com",
    "gist.github.com",
];

pub const TORRENT_DOMAINS: &[&str] = &[
    "rutracker.org",
    "rutracker.cc",
    "rutracker.net",
    "static.rutracker.cc",
    "kinozal.tv",
    "kinozal-tv.org",
    "rutor.info",
    "rutor.is",
    "d.rutor.info",
    "flibusta.is",
    "flibusta.site",
    "flibusta.me",
    "nnmclub.to",
    "torlook.info",
    "hdrezka.ag",
    "lostfilm.tv",
];

pub const ADULT_DOMAINS: &[&str] = &[
    "pornhub.com",
    "rt.pornhub.com",
    "phncdn.com",
    "phprcdn.com",
    "xvideos.com",
    "xvideos-cdn.com",
    "xnxx.com",
    "xnxx-cdn.com",
    "xhamster.com",
    "xhamster.desi",
    "xhcdn.com",
    "redtube.com",
    "redtube.net",
    "youporn.com",
    "ypncdn.com",
    "eporner.com",
    "bravoteens.com",
    "hqporner.com",
    "spankbang.com",
    "chaturbate.com",
    "bongacams.com",
    "stripchat.com",
];

pub const ISOLATED_PROXIED_DOMAINS: &[&str] = &[
    "openai.com",
    "chatgpt.com",
    "oaistatic.com",
    "oaiusercontent.com",
    "anthropic.com",
    "claude.ai",
    "instagram.com",
    "cdninstagram.com",
    "twitter.com",
    "x.com",
    "twimg.com",
];

pub const FLOW_DOMAINS: &[&str] = &[
    "google.com",
    "google.dev",
    "googleapis.com",
    "googleusercontent.com",
    "gstatic.com",
    "flow.google.com",
    "labs.google",
    "aisandbox-pa.googleapis.com",
    "aisandbox-pa.google",
    "alkalimakersuite-pa.googleapis.com",
    "content-alkalimakersuite-pa.googleapis.com",
    "alkalimakersuite-pa.clients6.google.com",
    "waa-pa.clients6.google.com",
    "waa-pa.googleapis.com",
    "generativelanguage.googleapis.com",
    "aistudio.google.com",
    "aistudio.google",
    "gemini.google.com",
    "gemini.google",
    "bard.google.com",
    "makersuite.google.com",
    "proactivebackend-pa.googleapis.com",
    "cloudaicompanion.googleapis.com",
    "cloudcode-pa.googleapis.com",
    "daily-cloudcode-pa.googleapis.com",
    "developerconnect.googleapis.com",
    "deepmind.google",
    "deepmind.com",
    "notebooklm.google",
    "notebooklm.google.com",
    "ai.google.dev",
    "ai.google",
    "genai-media.googleusercontent.com",
    "video-downloads.googleusercontent.com",
    "anthropic.com",
    "claude.ai",
    "openai.com",
    "chatgpt.com",
    "oaistatic.com",
    "oaiusercontent.com",
];

/// Проверяет, обрабатывается ли домен локальным обходом Zapret (DIRECT через nfqws).
/// Защищенные Google Flow & AI домены и изолированные PROXY сервисы всегда исключаются.
pub fn is_zapret_direct_domain(domain: &str, zapret_cfg: &crate::config::ZapretConfig) -> bool {
    if !zapret_cfg.enabled {
        return false;
    }
    let d = domain.trim().to_lowercase();
    // Flow/Google AI домены всегда защищены и должны идти через PROXY
    if FLOW_DOMAINS.iter().any(|g| d == *g || d.ends_with(&format!(".{}", g))) {
        return false;
    }
    // Изолированные сервисы всегда идут через PROXY при активной изоляции
    if zapret_cfg.isolated_proxy && ISOLATED_PROXIED_DOMAINS.iter().any(|p| d == *p || d.ends_with(&format!(".{}", p))) {
        return false;
    }

    let matches_list = |list: &[&str]| -> bool {
        list.iter().any(|item| {
            let item_lower = item.trim().to_lowercase();
            d == item_lower || d.ends_with(&format!(".{}", item_lower))
        })
    };

    if (zapret_cfg.hybrid_youtube || zapret_cfg.smart_tv_mode) && matches_list(YOUTUBE_HYBRID_DOMAINS) {
        return true;
    }
    if zapret_cfg.smart_tv_mode && matches_list(SMART_TV_GGC_DOMAINS) {
        return true;
    }
    if zapret_cfg.hybrid_discord && matches_list(DISCORD_HYBRID_DOMAINS) {
        return true;
    }
    if zapret_cfg.bypass_github && matches_list(GITHUB_DOMAINS) {
        return true;
    }
    if zapret_cfg.bypass_torrents && matches_list(TORRENT_DOMAINS) {
        return true;
    }
    if zapret_cfg.bypass_adult && matches_list(ADULT_DOMAINS) {
        return true;
    }
    for entry in &zapret_cfg.custom_entries {
        if entry.enabled {
            let c = crate::config::normalize_domain(&entry.domain);
            if !c.is_empty() && (d == c || d.ends_with(&format!(".{}", c))) {
                return true;
            }
            for cdn in &entry.cdns {
                let c_cdn = crate::config::normalize_domain(cdn);
                if !c_cdn.is_empty() && (d == c_cdn || d.ends_with(&format!(".{}", c_cdn))) {
                    return true;
                }
            }
        }
    }
    false
}

/// Удаление блока Google Flow & AI правил из YAML.
pub fn remove_flow_rules(yaml: &str) -> String {
    if !yaml.contains(GOOGLE_AI_BEGIN) {
        return yaml.to_string();
    }
    if !yaml.contains(GOOGLE_AI_END) {
        return yaml.lines().filter(|l| l.trim() != GOOGLE_AI_BEGIN).collect::<Vec<_>>().join("\n");
    }
    let mut out = Vec::new();
    let mut skip = false;
    for line in yaml.lines() {
        let t = line.trim();
        if t == GOOGLE_AI_BEGIN {
            skip = true;
            continue;
        }
        if t == GOOGLE_AI_END {
            skip = false;
            continue;
        }
        if !skip {
            out.push(line);
        }
    }
    out.join("\n")
}

/// Поиск существующей группы Flow / Google AI в YAML конфиге Mihomo.
pub fn find_flow_group_name(yaml: &str) -> Option<String> {
    for gname in ["Google AI", "Google-AI", "GoogleFlow", "Flow", "AI"] {
        let pattern_unquoted = format!("- name: {gname}");
        let pattern_single = format!("- name: '{gname}'");
        let pattern_double = format!("- name: \"{gname}\"");
        for line in yaml.lines() {
            let t = line.trim();
            if t == pattern_unquoted
                || t == pattern_single
                || t == pattern_double
                || t.starts_with(&format!("- name: {gname} "))
                || t.starts_with(&format!("- name: '{gname}'"))
                || t.starts_with(&format!("- name: \"{gname}\""))
            {
                return Some(gname.to_string());
            }
        }
    }
    None
}

/// Применение выделенного маршрута Google Flow & AI в rules: (после AdBlock).
pub fn apply_flow_rules(yaml: &str, target: &str, group_name: Option<&str>) -> Result<String, String> {
    crate::routing::validate_marker_pair(yaml, GOOGLE_AI_BEGIN, GOOGLE_AI_END)?;
    let forbidden_chars = ['\r', '\n', '"', '\'', ','];
    if target.chars().any(|c| forbidden_chars.contains(&c))
        || group_name.map(|g| g.chars().any(|c| forbidden_chars.contains(&c))).unwrap_or(false)
    {
        return Err("Параметры правила Flow содержат недопустимые символы (переводы строк, кавычки или запятые)".into());
    }
    let content = remove_flow_rules(yaml);
    let target_dest = if let Some(g) = group_name { g } else { target };
    if target_dest.is_empty() {
        return Ok(content);
    }
    let lines: Vec<&str> = content.lines().collect();
    let rules_idx = lines
        .iter()
        .position(|l| l.trim_end() == "rules:")
        .ok_or("В config.yaml нет секции rules:")?;

    let mut out = Vec::with_capacity(lines.len() + FLOW_DOMAINS.len() + 4);
    for (i, line) in lines.iter().enumerate() {
        out.push(line.to_string());
        if i == rules_idx {
            out.push(GOOGLE_AI_BEGIN.to_string());
            for d in FLOW_DOMAINS {
                out.push(format!("  - DOMAIN-SUFFIX,{d},{target_dest}"));
            }
            out.push(GOOGLE_AI_END.to_string());
        }
    }
    Ok(out.join("\n"))
}

/// Удаление блока AdBlock из YAML.
pub fn remove_adblock_rules(yaml: &str) -> String {
    if !yaml.contains(ADBLOCK_BEGIN) {
        return yaml.to_string();
    }
    if !yaml.contains(ADBLOCK_END) {
        return yaml.lines().filter(|l| l.trim() != ADBLOCK_BEGIN).collect::<Vec<_>>().join("\n");
    }
    let mut out = Vec::new();
    let mut skip = false;
    for line in yaml.lines() {
        let t = line.trim();
        if t == ADBLOCK_BEGIN {
            skip = true;
            continue;
        }
        if t == ADBLOCK_END {
            skip = false;
            continue;
        }
        if !skip {
            out.push(line);
        }
    }
    out.join("\n")
}

/// Применение правила AdBlock: если включено — вставка сразу под rules:.
/// Если выключено — удаление блока.
pub fn apply_adblock_rules(yaml: &str, enabled: bool) -> Result<String, String> {
    crate::routing::validate_marker_pair(yaml, ADBLOCK_BEGIN, ADBLOCK_END)?;
    let content = remove_adblock_rules(yaml);
    if !enabled {
        return Ok(content);
    }
    let lines: Vec<&str> = content.lines().collect();
    let rules_idx = lines
        .iter()
        .position(|l| l.trim_end() == "rules:")
        .ok_or("В config.yaml нет секции rules:")?;

    let mut out = Vec::with_capacity(lines.len() + 4);
    for (i, line) in lines.iter().enumerate() {
        out.push(line.to_string());
        if i == rules_idx {
            out.push(ADBLOCK_BEGIN.to_string());
            out.push(ADBLOCK_RULE.to_string());
            out.push(ADBLOCK_END.to_string());
        }
    }
    Ok(out.join("\n"))
}

/// Чистое удаление инжектированных адресов AdGuard Home из секции DNS
pub fn remove_adguard_dns_upstream(yaml: &str) -> String {
    let mut out = Vec::new();
    let mut skip_dns = false;
    let mut skip_direct = false;
    for line in yaml.lines() {
        let t = line.trim();
        if t == AGH_DNS_BEGIN {
            skip_dns = true;
            continue;
        }
        if t == AGH_DNS_END {
            skip_dns = false;
            continue;
        }
        if t == AGH_DIRECT_DNS_BEGIN {
            skip_direct = true;
            continue;
        }
        if t == AGH_DIRECT_DNS_END {
            skip_direct = false;
            continue;
        }
        if !skip_dns && !skip_direct {
            out.push(line);
        }
    }
    out.join("\n")
}

/// Применение интеграции AdGuard Home в секцию DNS ядра Mihomo (nameserver / direct-nameserver).
/// Перенаправляет весь DNS-трафик под прокси в локальный AdGuard Home (127.0.0.1:{agh_port}).
pub fn apply_adguard_dns_upstream(yaml: &str, agh_port: u16, enabled: bool) -> Result<String, String> {
    let content = remove_adguard_dns_upstream(yaml);
    if !enabled || agh_port == 0 {
        return Ok(content);
    }

    let port = if agh_port > 0 { agh_port } else { 5354 };
    let agh_addr = format!("127.0.0.1:{}", port);

    let lines: Vec<&str> = content.lines().collect();
    let mut out = Vec::with_capacity(lines.len() + 8);
    let mut injected_nameserver = false;
    let mut injected_direct = false;

    for line in lines {
        out.push(line.to_string());
        let trimmed = line.trim();

        if trimmed == "nameserver:" && !injected_nameserver {
            let base_indent = line.len() - line.trim_start().len();
            let item_indent = " ".repeat(base_indent + 2);
            out.push(format!("{}{}", item_indent, AGH_DNS_BEGIN));
            out.push(format!("{}- {}", item_indent, agh_addr));
            out.push(format!("{}{}", item_indent, AGH_DNS_END));
            injected_nameserver = true;
        } else if trimmed == "direct-nameserver:" && !injected_direct {
            let base_indent = line.len() - line.trim_start().len();
            let item_indent = " ".repeat(base_indent + 2);
            out.push(format!("{}{}", item_indent, AGH_DIRECT_DNS_BEGIN));
            out.push(format!("{}- {}", item_indent, agh_addr));
            out.push(format!("{}{}", item_indent, AGH_DIRECT_DNS_END));
            injected_direct = true;
        }
    }

    Ok(out.join("\n"))
}

/// Удаление блока правил Zapret Hybrid из YAML.
pub fn remove_zapret_hybrid_rules(yaml: &str) -> String {
    if !yaml.contains(ZAPRET_HYBRID_BEGIN) {
        return yaml.to_string();
    }
    if !yaml.contains(ZAPRET_HYBRID_END) {
        return yaml.lines().filter(|l| l.trim() != ZAPRET_HYBRID_BEGIN).collect::<Vec<_>>().join("\n");
    }
    let mut out = Vec::new();
    let mut skip = false;
    for line in yaml.lines() {
        let t = line.trim();
        if t == ZAPRET_HYBRID_BEGIN {
            skip = true;
            continue;
        }
        if t == ZAPRET_HYBRID_END {
            skip = false;
            continue;
        }
        if !skip {
            out.push(line);
        }
    }
    out.join("\n")
}

/// Поиск имени основной прокси-группы в YAML (PROXY, Proxy, Fastest, Auto, Fallback и т.д.).
pub fn find_proxy_target_group(yaml: &str) -> String {
    for gname in ["PROXY", "Proxy", "proxy", "Google AI", "Google-AI", "GoogleFlow", "Flow", "AI", "Fastest", "Auto", "Fallback"] {
        let pattern_unquoted = format!("- name: {gname}");
        let pattern_single = format!("- name: '{gname}'");
        let pattern_double = format!("- name: \"{gname}\"");
        for line in yaml.lines() {
            let t = line.trim();
            if t == pattern_unquoted
                || t == pattern_single
                || t == pattern_double
                || t.starts_with(&format!("- name: {gname} "))
                || t.starts_with(&format!("- name: '{gname}'"))
                || t.starts_with(&format!("- name: \"{gname}\""))
            {
                return gname.to_string();
            }
        }
    }
    for g in parse_existing_group_names(yaml) {
        if g != "DIRECT" && g != "REJECT" && g != "GLOBAL" && g != GAMING_GROUP_NAME {
            return g;
        }
    }
    "PROXY".to_string()
}

/// Применение правил Zapret Hybrid (YouTube DIRECT, Discord DIRECT, Изоляция PROXY).
pub fn apply_zapret_hybrid_rules(yaml: &str, zapret_cfg: &crate::config::ZapretConfig) -> Result<String, String> {
    apply_zapret_hybrid_rules_with_force(yaml, zapret_cfg, &[])
}

/// Применение правил Zapret Hybrid с учетом пользовательских принудительных доменов (force_domains).
/// Любой домен, явно указанный пользователем в force_domains (PROXY), имеет абсолютный приоритет
/// и исключается из автоматических DIRECT-правил Zapret.
pub fn apply_zapret_hybrid_rules_with_force(
    yaml: &str,
    zapret_cfg: &crate::config::ZapretConfig,
    force_domains: &[String],
) -> Result<String, String> {
    crate::routing::validate_marker_pair(yaml, ZAPRET_HYBRID_BEGIN, ZAPRET_HYBRID_END)?;
    let content = remove_zapret_hybrid_rules(yaml);
    if !zapret_cfg.enabled {
        return Ok(content);
    }

    let is_forced = |domain: &str| -> bool {
        let d = crate::config::normalize_domain(domain);
        if d.is_empty() {
            return false;
        }
        force_domains.iter().any(|f| {
            let clean_f = crate::config::normalize_domain(f);
            !clean_f.is_empty() && (d == clean_f || d.ends_with(&format!(".{clean_f}")) || clean_f.ends_with(&format!(".{d}")))
        })
    };

    let proxy_target = find_proxy_target_group(yaml);
    let mut rules_to_add: Vec<String> = Vec::new();

    // 0. Safeguard для Google AI / Antigravity (гарантированный PROXY перед любыми DIRECT правилами)
    let has_active_custom = zapret_cfg.custom_entries.iter().any(|e| e.enabled);
    if zapret_cfg.isolated_proxy
        || zapret_cfg.hybrid_youtube
        || zapret_cfg.hybrid_discord
        || zapret_cfg.bypass_github
        || zapret_cfg.bypass_torrents
        || zapret_cfg.bypass_adult
        || zapret_cfg.smart_tv_mode
        || has_active_custom
    {
        for d in FLOW_DOMAINS {
            let rule = format!("  - DOMAIN-SUFFIX,{d},{proxy_target}");
            if !rules_to_add.contains(&rule) {
                rules_to_add.push(rule);
            }
        }
    }

    // 1. Изоляция IP-блокировок (ChatGPT, Claude, X/Twitter, Instagram -> PROXY)
    if zapret_cfg.isolated_proxy {
        for d in ISOLATED_PROXIED_DOMAINS {
            let rule = format!("  - DOMAIN-SUFFIX,{d},{proxy_target}");
            if !rules_to_add.contains(&rule) {
                rules_to_add.push(rule);
            }
        }
    }

    // 2. YouTube -> DIRECT (максимальная скорость с локальных кэшей GGC)
    if zapret_cfg.hybrid_youtube {
        for d in YOUTUBE_HYBRID_DOMAINS {
            if !is_forced(d) {
                let rule = format!("  - DOMAIN-SUFFIX,{d},DIRECT");
                if !rules_to_add.contains(&rule) {
                    rules_to_add.push(rule);
                }
            }
        }
    }

    // 2.1 Smart TV профиль -> DIRECT (оптимизация всех CDN/GGC кэшей)
    if zapret_cfg.smart_tv_mode {
        for d in SMART_TV_GGC_DOMAINS {
            if !is_forced(d) {
                let rule = format!("  - DOMAIN-SUFFIX,{d},DIRECT");
                if !rules_to_add.contains(&rule) {
                    rules_to_add.push(rule);
                }
            }
        }
    }

    // 3. Discord -> DIRECT (минимальный пинг, прямые шлюзы)
    if zapret_cfg.hybrid_discord {
        for d in DISCORD_HYBRID_DOMAINS {
            if !is_forced(d) {
                let rule = format!("  - DOMAIN-SUFFIX,{d},DIRECT");
                if !rules_to_add.contains(&rule) {
                    rules_to_add.push(rule);
                }
            }
        }
    }

    // 4. GitHub -> DIRECT (высокая скорость git clone и релизов без расхода VPS)
    if zapret_cfg.bypass_github {
        for d in GITHUB_DOMAINS {
            if !is_forced(d) {
                let rule = format!("  - DOMAIN-SUFFIX,{d},DIRECT");
                if !rules_to_add.contains(&rule) {
                    rules_to_add.push(rule);
                }
            }
        }
    }

    // 5. Торренты & Трекеры -> DIRECT
    if zapret_cfg.bypass_torrents {
        for d in TORRENT_DOMAINS {
            if !is_forced(d) {
                let rule = format!("  - DOMAIN-SUFFIX,{d},DIRECT");
                if !rules_to_add.contains(&rule) {
                    rules_to_add.push(rule);
                }
            }
        }
    }

    // 6. 18+ Контент -> DIRECT
    if zapret_cfg.bypass_adult {
        for d in ADULT_DOMAINS {
            if !is_forced(d) {
                let rule = format!("  - DOMAIN-SUFFIX,{d},DIRECT");
                if !rules_to_add.contains(&rule) {
                    rules_to_add.push(rule);
                }
            }
        }
    }

    // 7. Пользовательские сайты и их связанные CDN (/boost) -> DIRECT
    for entry in &zapret_cfg.custom_entries {
        if entry.enabled {
            let clean = crate::config::normalize_domain(&entry.domain);
            if !clean.is_empty() && !FLOW_DOMAINS.contains(&clean.as_str()) && !is_forced(&clean) {
                let rule = format!("  - DOMAIN-SUFFIX,{clean},DIRECT");
                if !rules_to_add.contains(&rule) {
                    rules_to_add.push(rule);
                }
            }
            for cdn in &entry.cdns {
                let clean_cdn = crate::config::normalize_domain(cdn);
                if !clean_cdn.is_empty() && !FLOW_DOMAINS.contains(&clean_cdn.as_str()) && !is_forced(&clean_cdn) {
                    let rule = format!("  - DOMAIN-SUFFIX,{clean_cdn},DIRECT");
                    if !rules_to_add.contains(&rule) {
                        rules_to_add.push(rule);
                    }
                }
            }
        }
    }

    if rules_to_add.is_empty() {
        return Ok(content);
    }

    let lines: Vec<&str> = content.lines().collect();
    let rules_idx = lines
        .iter()
        .position(|l| l.trim_end() == "rules:")
        .ok_or("В config.yaml нет секции rules:")?;

    // Порядок вставки блока ZAPRET_HYBRID:
    // 1. Перед FORCE_BEGIN (если присутствует блок принудительного проксирования);
    // 2. Иначе после DIRECT_END (если присутствует блок прямого белого списка);
    // 3. Иначе после DEV_DOMAINS_END (если присутствуют индивидуальные правила устройств);
    // 4. Иначе сразу после rules: (в начале секции правил).
    // Это гарантирует четкую и неизменную иерархию правил Mihomo:
    // FLOW_DOMAINS > DEV_DOMAINS > DIRECT_DOMAINS > ZAPRET_HYBRID > FORCE_DOMAINS
    let force_idx = lines.iter().position(|l| l.trim() == FORCE_BEGIN);
    let direct_end_idx = lines.iter().position(|l| l.trim() == DIRECT_END);
    let dev_end_idx = lines.iter().position(|l| l.trim() == DEV_DOMAINS_END);

    let mut out = Vec::with_capacity(lines.len() + rules_to_add.len() + 4);
    if let Some(f_idx) = force_idx {
        for (i, line) in lines.iter().enumerate() {
            if i == f_idx {
                out.push(ZAPRET_HYBRID_BEGIN.to_string());
                for r in &rules_to_add {
                    out.push(r.clone());
                }
                out.push(ZAPRET_HYBRID_END.to_string());
            }
            out.push(line.to_string());
        }
    } else if let Some(d_end) = direct_end_idx {
        for (i, line) in lines.iter().enumerate() {
            out.push(line.to_string());
            if i == d_end {
                out.push(ZAPRET_HYBRID_BEGIN.to_string());
                for r in &rules_to_add {
                    out.push(r.clone());
                }
                out.push(ZAPRET_HYBRID_END.to_string());
            }
        }
    } else if let Some(dev_end) = dev_end_idx {
        for (i, line) in lines.iter().enumerate() {
            out.push(line.to_string());
            if i == dev_end {
                out.push(ZAPRET_HYBRID_BEGIN.to_string());
                for r in &rules_to_add {
                    out.push(r.clone());
                }
                out.push(ZAPRET_HYBRID_END.to_string());
            }
        }
    } else {
        for (i, line) in lines.iter().enumerate() {
            out.push(line.to_string());
            if i == rules_idx {
                out.push(ZAPRET_HYBRID_BEGIN.to_string());
                for r in &rules_to_add {
                    out.push(r.clone());
                }
                out.push(ZAPRET_HYBRID_END.to_string());
            }
        }
    }
    Ok(out.join("\n"))
}

/// Сбор всех активных доменов игрового режима из GamingConfig.
pub fn get_gaming_domains(cfg: &crate::config::GamingConfig) -> Vec<String> {
    if !cfg.enabled {
        return Vec::new();
    }
    let mut domains = Vec::new();
    let p = &cfg.platforms;
    if p.discord { domains.extend(GAMING_DISCORD_DOMAINS.iter().map(|&s| s.to_string())); }
    if p.steam { domains.extend(GAMING_STEAM_DOMAINS.iter().map(|&s| s.to_string())); }
    if p.playstation { domains.extend(GAMING_PSN_DOMAINS.iter().map(|&s| s.to_string())); }
    if p.xbox { domains.extend(GAMING_XBOX_DOMAINS.iter().map(|&s| s.to_string())); }
    if p.battlenet { domains.extend(GAMING_BATTLENET_DOMAINS.iter().map(|&s| s.to_string())); }
    if p.epicgames { domains.extend(GAMING_EPIC_DOMAINS.iter().map(|&s| s.to_string())); }
    if p.ea { domains.extend(GAMING_EA_DOMAINS.iter().map(|&s| s.to_string())); }
    if p.riot { domains.extend(GAMING_RIOT_DOMAINS.iter().map(|&s| s.to_string())); }
    if p.supercell { domains.extend(GAMING_SUPERCELL_DOMAINS.iter().map(|&s| s.to_string())); }
    if p.nintendo { domains.extend(GAMING_NINTENDO_DOMAINS.iter().map(|&s| s.to_string())); }
    if p.roblox { domains.extend(GAMING_ROBLOX_DOMAINS.iter().map(|&s| s.to_string())); }
    for c in &cfg.custom_domains {
        let t = c.trim().to_lowercase();
        if !t.is_empty() && !domains.contains(&t) {
            domains.push(t);
        }
    }
    domains
}

/// Удаление игровых правил и группы 🎮 Gaming из YAML.
pub fn remove_gaming_rules(yaml: &str) -> String {
    let mut content = yaml.to_string();

    // 1. Удаление блока правил в rules:
    if content.contains(GAMING_BEGIN) {
        if !content.contains(GAMING_END) {
            content = content.lines().filter(|l| l.trim() != GAMING_BEGIN).collect::<Vec<_>>().join("\n");
        } else {
            let mut out = Vec::new();
            let mut skip = false;
            for line in content.lines() {
                let t = line.trim();
                if t == GAMING_BEGIN {
                    skip = true;
                    continue;
                }
                if t == GAMING_END {
                    skip = false;
                    continue;
                }
                if !skip {
                    out.push(line);
                }
            }
            content = out.join("\n");
        }
    }

    // 2. Удаление селектор-группы в proxy-groups:
    if content.contains(GAMING_GROUP_BEGIN) {
        if !content.contains(GAMING_GROUP_END) {
            content = content.lines().filter(|l| l.trim() != GAMING_GROUP_BEGIN).collect::<Vec<_>>().join("\n");
        } else {
            let mut out = Vec::new();
            let mut skip = false;
            for line in content.lines() {
                let t = line.trim();
                if t == GAMING_GROUP_BEGIN {
                    skip = true;
                    continue;
                }
                if t == GAMING_GROUP_END {
                    skip = false;
                    continue;
                }
                if !skip {
                    out.push(line);
                }
            }
            content = out.join("\n");
        }
    }

    content
}

/// Имена proxy-groups верхнего уровня из config.yaml.
pub fn parse_existing_group_names(yaml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_section = false;
    for line in yaml.lines() {
        if line.trim_end() == "proxy-groups:" && !line.starts_with(' ') {
            in_section = true;
            continue;
        }
        if in_section {
            if !line.starts_with(' ') && !line.trim().is_empty() && !line.trim().starts_with('#') {
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

/// Применение выделенного маршрута игрового режима (селектор-группа 🎮 Gaming и правила rules:).
pub fn apply_gaming_rules(
    yaml: &str,
    cfg: &crate::config::GamingConfig,
    providers: &[String],
) -> Result<String, String> {
    crate::routing::validate_marker_pair(yaml, GAMING_BEGIN, GAMING_END)?;
    crate::routing::validate_marker_pair(yaml, GAMING_GROUP_BEGIN, GAMING_GROUP_END)?;
    let content = remove_gaming_rules(yaml);
    if !cfg.enabled {
        return Ok(content);
    }

    let target_srv = if cfg.target_server.trim().is_empty() {
        "Fastest"
    } else {
        cfg.target_server.trim()
    };

    let existing_groups = parse_existing_group_names(&content);
    let static_proxies = parse_static_proxy_names(&content);
    let has_providers_section = content.contains("proxy-providers:");

    // 1. Вставка селектор-группы в proxy-groups:
    let lines: Vec<&str> = content.lines().collect();
    let pg_idx = lines
        .iter()
        .position(|l| l.trim_end() == "proxy-groups:")
        .ok_or("В config.yaml нет секции proxy-groups:")?;

    let mut with_group = Vec::with_capacity(lines.len() + 16);
    for (i, line) in lines.iter().enumerate() {
        with_group.push(line.to_string());
        if i == pg_idx {
            with_group.push(GAMING_GROUP_BEGIN.to_string());
            with_group.push(format!("  - name: '{GAMING_GROUP_NAME}'"));
            with_group.push("    type: select".to_string());
            with_group.push("    proxies:".to_string());
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
                    || content.contains(&format!("- {p}"))
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
                    with_group.push(format!("      - '{}'", p.replace('\'', "''")));
                } else {
                    with_group.push(format!("      - {p}"));
                }
            }
            if !providers.is_empty() {
                with_group.push("    use:".to_string());
                for p in providers {
                    let p = p.trim();
                    if !p.is_empty() {
                        with_group.push(format!("      - {p}"));
                    }
                }
            }
            with_group.push(GAMING_GROUP_END.to_string());
        }
    }
    let intermediate = with_group.join("\n");

    // 2. Вставка правил в rules:
    let lines2: Vec<&str> = intermediate.lines().collect();
    let rules_idx = lines2
        .iter()
        .position(|l| l.trim_end() == "rules:")
        .ok_or("В config.yaml нет секции rules:")?;

    let mut gaming_rules = Vec::new();
    match cfg.mode {
        crate::config::GamingMode::Compatibility => {
            // Режим совместимости (100% Все в прокси): направляет весь интернет-трафик устройства через игровой туннель
            let mut routed_any = false;
            let has_explicit_enabled = cfg.devices.iter().any(|d| d.enabled);
            for dev in &cfg.devices {
                let is_active = if has_explicit_enabled {
                    dev.enabled
                } else {
                    cfg.devices.len() == 1
                };
                if !is_active {
                    continue;
                }
                let target = dev
                    .server
                    .as_ref()
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| s.as_str())
                    .unwrap_or(GAMING_GROUP_NAME);
                let ip = dev.ip.trim();
                if !ip.is_empty() {
                    let cidr = if ip.contains('/') { ip.to_string() } else { format!("{ip}/32") };
                    gaming_rules.push(format!("  - SRC-IP-CIDR,{cidr},{target}"));
                    routed_any = true;
                }
                for v6 in &dev.ipv6 {
                    let v6 = v6.trim();
                    let v6_lower = v6.to_lowercase();
                    if !v6.is_empty() && !v6_lower.starts_with("fe80:") && !v6_lower.starts_with("::1") {
                        let cidr = if v6.contains('/') { v6.to_string() } else { format!("{v6}/128") };
                        gaming_rules.push(format!("  - SRC-IP-CIDR,{cidr},{target}"));
                        routed_any = true;
                    }
                }
            }
            for c in &cfg.custom_domains {
                let t = c.trim().to_lowercase();
                if !t.is_empty() {
                    gaming_rules.push(format!("  - DOMAIN-SUFFIX,{t},{GAMING_GROUP_NAME}"));
                }
            }
            // Если устройства ещё не настроены, добавляем домены как fallback
            if !routed_any {
                let domains = get_gaming_domains(cfg);
                for d in &domains {
                    gaming_rules.push(format!("  - DOMAIN-SUFFIX,{d},{GAMING_GROUP_NAME}"));
                }
            }
        }
        crate::config::GamingMode::BypassRu => {
            // Режим «Все в прокси кроме RU»:
            // Для каждого активного устройства исключаем российские домены и GeoIP,
            // а весь остальной трафик направляем в туннель/сервер
            let mut routed_any = false;
            let has_explicit_enabled = cfg.devices.iter().any(|d| d.enabled);
            let has_ru_ruleset = content.contains("category-ru@domain");
            let has_ip_ruleset = content.contains("category-ru@ipcidr");
            for dev in &cfg.devices {
                let is_active = if has_explicit_enabled {
                    dev.enabled
                } else {
                    cfg.devices.len() == 1
                };
                if !is_active {
                    continue;
                }
                let target = dev
                    .server
                    .as_ref()
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| s.as_str())
                    .unwrap_or(GAMING_GROUP_NAME);
                let ip = dev.ip.trim();
                if !ip.is_empty() {
                    let cidr = if ip.contains('/') { ip.to_string() } else { format!("{ip}/32") };
                    gaming_rules.push(format!("  - AND,((SRC-IP-CIDR,{cidr}),(GEOIP,RU)),DIRECT"));
                    if has_ru_ruleset {
                        gaming_rules.push(format!("  - AND,((SRC-IP-CIDR,{cidr}),(RULE-SET,category-ru@domain)),DIRECT"));
                        if has_ip_ruleset {
                            gaming_rules.push(format!("  - AND,((SRC-IP-CIDR,{cidr}),(RULE-SET,category-ru@ipcidr)),DIRECT"));
                        }
                    } else {
                        gaming_rules.push(format!("  - AND,((SRC-IP-CIDR,{cidr}),(GEOSITE,category-ru)),DIRECT"));
                    }
                    gaming_rules.push(format!("  - SRC-IP-CIDR,{cidr},{target}"));
                    routed_any = true;
                }
                for v6 in &dev.ipv6 {
                    let v6 = v6.trim();
                    let v6_lower = v6.to_lowercase();
                    if !v6.is_empty() && !v6_lower.starts_with("fe80:") && !v6_lower.starts_with("::1") {
                        let cidr = if v6.contains('/') { v6.to_string() } else { format!("{v6}/128") };
                        gaming_rules.push(format!("  - AND,((SRC-IP-CIDR,{cidr}),(GEOIP,RU)),DIRECT"));
                        if has_ru_ruleset {
                            gaming_rules.push(format!("  - AND,((SRC-IP-CIDR,{cidr}),(RULE-SET,category-ru@domain)),DIRECT"));
                            if has_ip_ruleset {
                                gaming_rules.push(format!("  - AND,((SRC-IP-CIDR,{cidr}),(RULE-SET,category-ru@ipcidr)),DIRECT"));
                            }
                        } else {
                            gaming_rules.push(format!("  - AND,((SRC-IP-CIDR,{cidr}),(GEOSITE,category-ru)),DIRECT"));
                        }
                        gaming_rules.push(format!("  - SRC-IP-CIDR,{cidr},{target}"));
                        routed_any = true;
                    }
                }
            }
            for c in &cfg.custom_domains {
                let t = c.trim().to_lowercase();
                if !t.is_empty() {
                    gaming_rules.push(format!("  - DOMAIN-SUFFIX,{t},{GAMING_GROUP_NAME}"));
                }
            }
            if !routed_any {
                let domains = get_gaming_domains(cfg);
                for d in &domains {
                    gaming_rules.push(format!("  - DOMAIN-SUFFIX,{d},{GAMING_GROUP_NAME}"));
                }
            }
        }
        crate::config::GamingMode::KnownServices | crate::config::GamingMode::SmartSplit => {
            // Режим «Все напрямую кроме игр» на базе category-games и правил платформ
            let domains = get_gaming_domains(cfg);
            for d in &domains {
                gaming_rules.push(format!("  - DOMAIN-SUFFIX,{d},{GAMING_GROUP_NAME}"));
            }
            if cfg.platforms.category_games {
                gaming_rules.push(format!("  - GEOSITE,category-games,{GAMING_GROUP_NAME}"));
            }
            for c in &cfg.custom_domains {
                let t = c.trim().to_lowercase();
                if !t.is_empty() {
                    gaming_rules.push(format!("  - DOMAIN-SUFFIX,{t},{GAMING_GROUP_NAME}"));
                }
            }
        }
    }

    let mut out = Vec::with_capacity(lines2.len() + gaming_rules.len() + 4);
    for (i, line) in lines2.iter().enumerate() {
        out.push(line.to_string());
        if i == rules_idx {
            out.push(GAMING_BEGIN.to_string());
            for r in &gaming_rules {
                out.push(r.clone());
            }
            out.push(GAMING_END.to_string());
        }
    }

    Ok(out.join("\n"))
}

/// Возвращает полный список доменов белого списка (376 базовых отечественных сервисов + пользовательские домены).
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
///    - Отечественные домены (376 базовых + custom) -> DIRECT (строго без GEOIP-RU)
///    - Трафик назначенных устройств -> 🛡️ Whitelist (или выбранный target_server)
/// Разрешение IP-адресов исключенных из белых списков устройств (по IP или MAC).
pub fn resolve_excluded_whitelist_ips(cfg: &crate::config::AppConfig) -> Vec<String> {
    let mut ips = Vec::new();
    for item in &cfg.whitelist.excluded_devices {
        let clean = item.trim();
        if clean.is_empty() {
            continue;
        }
        if crate::config::is_valid_ip_or_cidr(clean) {
            ips.push(clean.to_string());
        } else {
            if let Some(dev) = cfg.gaming.devices.iter().find(|d| d.mac.eq_ignore_ascii_case(clean)) {
                if !dev.ip.trim().is_empty() {
                    ips.push(dev.ip.trim().to_string());
                }
            }
            if let Some(dev) = cfg.whitelist.devices.iter().find(|d| d.mac.eq_ignore_ascii_case(clean)) {
                if !dev.ip.trim().is_empty() {
                    ips.push(dev.ip.trim().to_string());
                }
            }
            if let Some((ip, _)) = cfg.device_routing.iter().find(|(k, _)| k.eq_ignore_ascii_case(clean)) {
                ips.push(ip.clone());
            }
        }
    }
    ips.sort();
    ips.dedup();
    ips
}

/// Применение правил режима белых списков:
/// 1. Вставка селектор-группы '🛡️ Whitelist' в proxy-groups:
/// 2. Вставка блока # --- AUTO-WHITELIST-BEGIN --- в rules:
///    - Отечественные домены (376 базовых + custom) -> DIRECT (строго без GEOIP-RU)
///    - При стратегии StrictXKeen:
///      * Исключенные устройства -> DIRECT
///      * Все остальные клиенты сети (Zero-Touch) -> 🛡️ Whitelist (или выбранный target_server)
///    - При стратегии Standard: только селектор 🛡️ Whitelist без тотального перехвата
/// Размещается непосредственно над AUTO-DEVICE-RULES.
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

    // 2. Формирование правил rules:
    // Отечественные домены -> DIRECT
    // Устройства -> WHITELIST_GROUP_NAME (или явный target_server)
    // ВАЖНО: Никакого GEOIP,RU wildcard!
    let mut wl_rules = Vec::new();
    wl_rules.push(WHITELIST_BEGIN.to_string());

    let all_domains = get_all_whitelist_domains(&cfg.whitelist);
    for dom in &all_domains {
        wl_rules.push(format!("  - DOMAIN-SUFFIX,{dom},DIRECT"));
    }

    // При стратегии StrictXKeen маршрутизируем всех клиентов сети в защищенный туннель (Zero-Touch)
    if cfg.whitelist.strategy == crate::config::WhitelistStrategy::StrictXKeen {
        let dev_target = match cfg.whitelist.target_server.as_deref() {
            Some(s) if !s.trim().is_empty() && s.trim() != "follow_failover" => s.trim(),
            _ => WHITELIST_GROUP_NAME,
        };

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

        // 2.2. Защита отечественного сегмента (RU домены и IP) для всех устройств сети и мобилок:
        // Гарантирует, что мобильные приложения (банки, маркетплейсы, госуслуги, CDN, push)
        // не перехватываются туннелем в зарубежный IP и работают напрямую без предупреждений «Обнаружен VPN».
        let has_ru_ruleset = with_group.contains("category-ru@domain");
        let has_ip_ruleset = with_group.contains("category-ru@ipcidr");
        if with_group.contains("category-ip-geo-detect@domain") {
            wl_rules.push("  - RULE-SET,category-ip-geo-detect@domain,DIRECT".to_string());
        }
        if has_ru_ruleset {
            if has_ip_ruleset {
                wl_rules.push("  - OR,((RULE-SET,category-ru@domain),(RULE-SET,category-ru@ipcidr)),DIRECT".to_string());
            } else {
                wl_rules.push("  - RULE-SET,category-ru@domain,DIRECT".to_string());
            }
        } else if with_group.contains("geosite") || with_group.contains("GEOSITE") {
            wl_rules.push("  - GEOSITE,category-ru,DIRECT".to_string());
        }

        // 2.3. Явно сконфигурированные устройства из whitelist.devices (если есть).
        // BS-PIN: устройство с закреплённой нодой получает СВОЁ правило (выше catch-all
        // 2.4), поэтому его трафик идёт через фиксированный узел и не зависит от
        // переключений Failover сервера БС (Failover управляет только группой Whitelist).
        for dev in &cfg.whitelist.devices {
            if dev.enabled && !dev.ip.trim().is_empty() && !excluded_ips.iter().any(|ex| ex == dev.ip.trim()) {
                let ip_trim = dev.ip.trim();
                let cidr = if ip_trim.contains('/') {
                    ip_trim.to_string()
                } else {
                    format!("{ip_trim}/32")
                };
                let pinned = dev.server.as_deref().map(str::trim).filter(|s| !s.is_empty());
                let target = pinned.unwrap_or(dev_target);
                let r = format!("  - SRC-IP-CIDR,{cidr},{target}");
                if !wl_rules.contains(&r) {
                    wl_rules.push(r);
                }
            }
        }

        // 2.4. Zero-Touch: Все клиенты локальных сетей роутера -> в туннель Белых списков (для всего внешнего трафика)
        for subnet in &["192.168.0.0/16", "10.0.0.0/8", "172.16.0.0/12"] {
            wl_rules.push(format!("  - SRC-IP-CIDR,{subnet},{dev_target}"));
        }
    }

    wl_rules.push(WHITELIST_END.to_string());

    // 3. Вставка в rules: непосредственно над AUTO-DEVICE-RULES (или перед fallback/в конце rules)
    let lines: Vec<&str> = with_group.lines().collect();
    let rules_pos = match lines.iter().position(|l| l.trim_end() == "rules:") {
        Some(pos) => pos,
        None => return with_group,
    };

    let target_insert_idx = if let Some(dev_rules_idx) = lines[rules_pos + 1..].iter().position(|l| l.trim() == RULES_BEGIN) {
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
    let active_devices: Vec<&crate::config::WhitelistDevice> = cfg
        .whitelist
        .devices
        .iter()
        .filter(|d| d.enabled && !d.ip.trim().is_empty())
        .collect();

    if !cfg.whitelist.enabled || active_devices.is_empty() {
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
    for (i, line) in lines.iter().enumerate() {
        out.push(line.to_string());
        if i == rules_idx {
            // 1. Персональные доменные правила устройств (наивысший приоритет)
            if has_dev_domains {
                out.push(DEV_DOMAINS_BEGIN.to_string());
                for (ip, rules) in device_domains {
                    let cidr = format_src_ip_cidr(ip);
                    for r in rules {
                        let dom = r.domain.trim().to_lowercase();
                        let target = r.target.trim();
                        if !dom.is_empty() && !target.is_empty() {
                            out.push(format!(
                                "  - AND,((SRC-IP-CIDR,{cidr}),(DOMAIN-SUFFIX,{dom})),{target},no-resolve"
                            ));
                        }
                    }
                }
                out.push(DEV_DOMAINS_END.to_string());
            }

            // 2. Прямые домены (DIRECT)
            if !direct.is_empty() {
                out.push(DIRECT_BEGIN.to_string());
                for d in &direct {
                    out.push(format!("  - DOMAIN-SUFFIX,{d},DIRECT"));
                }
                out.push(DIRECT_END.to_string());
            }

            // 3. Принудительные домены (динамическая прокси-группа, исключая специализированные Google AI домены)
            if !force.is_empty() {
                let proxy_target = find_proxy_target_group(&content);
                out.push(FORCE_BEGIN.to_string());
                for d in &force {
                    if !FLOW_DOMAINS.iter().any(|g| g.eq_ignore_ascii_case(d)) {
                        out.push(format!("  - DOMAIN-SUFFIX,{d},{proxy_target}"));
                    }
                }
                out.push(FORCE_END.to_string());
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
        let gname = group_name_for(ip, &a.name);
        groups_by_ip.insert(ip.to_string(), group_yaml(&gname, providers));
        rules_by_ip.insert(ip.to_string(), rule_line(ip, &gname));
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

/// Извлечение порта прозрачного перехвата (redir-port) из config.yaml Mihomo
pub fn parse_redir_port(yaml: &str) -> Option<u16> {
    for line in yaml.lines() {
        let t = line.trim();
        if t.starts_with("redir-port:") {
            if let Some(val_str) = t.strip_prefix("redir-port:") {
                let s = val_str.trim().trim_matches('\'').trim_matches('"');
                if let Ok(p) = s.parse::<u16>() {
                    return Some(p);
                }
            }
        }
    }
    None
}

/// Извлечение порта TPROXY перехвата (tproxy-port) из config.yaml Mihomo
pub fn parse_tproxy_port(yaml: &str) -> Option<u16> {
    for line in yaml.lines() {
        let t = line.trim();
        if t.starts_with("tproxy-port:") {
            if let Some(val_str) = t.strip_prefix("tproxy-port:") {
                let s = val_str.trim().trim_matches('\'').trim_matches('"');
                if let Ok(p) = s.parse::<u16>() {
                    return Some(p);
                }
            }
        }
    }
    None
}

/// Очистка устаревших ipset списков при смене режимов маршрутизации (BUG-19).
pub async fn cleanup_stale_mode_ipsets() -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        for set in &["xkeen_stale_temp", "xkeen_gaming_stale"] {
            let _ = tokio::process::Command::new("ipset")
                .args(["flush", set])
                .output()
                .await;
            let _ = tokio::process::Command::new("ipset")
                .args(["destroy", set])
                .output()
                .await;
        }
    }
    Ok(())
}

/// Применение всей сохраненной в AppConfig маршрутизации к сырому YAML Mihomo с сохранением фильтров провайдеров.
pub fn apply_routing_with_filters(
    yaml: &str,
    cfg: &crate::config::AppConfig,
    filters: &mut std::collections::BTreeMap<String, String>,
) -> Result<(String, usize), String> {
    let mut current = sync_disabled_providers_in_yaml(yaml, &cfg.disabled_providers).unwrap_or_else(|_| yaml.to_string());

    let providers = if !cfg.mihomo.device_providers.is_empty() {
        cfg.mihomo.device_providers.clone()
    } else {
        parse_provider_names(&current)
    };
    let providers: Vec<String> = providers
        .into_iter()
        .filter(|p| !cfg.disabled_providers.iter().any(|dp| dp.eq_ignore_ascii_case(p)))
        .collect();

    // 1. Per-device назначения (базовый маршрут уровня устройства: SRC-IP-CIDR)
    // Вставляется ПЕРВЫМ в rules, чтобы более специфичные правила (домены, AdBlock, Zapret, Flow, Gaming)
    // вставлялись поверх него и имели безусловный приоритет (BUG-18 FIFO-порядок).
    let mut assignments = Vec::new();
    for (ip, dr) in &cfg.device_routing {
        if let Some(srv) = dr.servers.first() {
            assignments.push(Assignment {
                ip: ip.clone(),
                name: String::new(),
                server: Some(srv.clone()),
            });
        }
    }
    // Включаем устройства с активными расписаниями (чтобы группа существовала для переключения)
    for s in &cfg.schedules {
        if s.enabled && !assignments.iter().any(|a| a.ip == s.ip) {
            let default_target = s.target_server.as_deref().unwrap_or("DIRECT");
            assignments.push(Assignment {
                ip: s.ip.clone(),
                name: String::new(),
                server: Some(default_target.to_string()),
            });
        }
    }
    // Включаем игровые устройства с выделенным сервером
    let has_explicit_gaming = cfg.gaming.devices.iter().any(|d| d.enabled);
    for d in &cfg.gaming.devices {
        let is_active = if has_explicit_gaming { d.enabled } else { cfg.gaming.devices.len() == 1 };
        if is_active {
            if let Some(ref srv) = d.server {
                if !srv.trim().is_empty() && !assignments.iter().any(|a| a.ip == d.ip) {
                    assignments.push(Assignment {
                        ip: d.ip.clone(),
                        name: d.name.clone(),
                        server: Some(srv.clone()),
                    });
                }
            }
        }
    }

    let count = assignments.len();
    if let Ok(with_devices) = apply_assignments(&current, &assignments, &providers) {
        current = with_devices;
    }

    // 1.1. Режим белых списков (376 отечественных сервисов напрямую, прокси-туннель для устройств)
    // Размещается непосредственно над AUTO-DEVICE-RULES и под AdBlock
    let active_wl_tunnel = match cfg.whitelist.target_server.as_deref() {
        Some(s) if !s.trim().is_empty() && s.trim() != "follow_failover" => Some(s.trim()),
        _ => None,
    };
    current = apply_whitelist_rules(&current, cfg, active_wl_tunnel);
    current = apply_whitelist_dns(&current, cfg, "127.0.0.1:53");

    // 2. Игнор-лист
    if let Ok(with_ig) = apply_ignore_to_groups(&current, &cfg.ignore_servers) {
        current = with_ig;
    }
    current = apply_ignore_to_providers(&current, &cfg.ignore_servers, filters);

    // 3. Доменные правила (DIRECT / FORCE / PER-DEVICE DOMAINS)
    // Вставляются поверх правил устройств
    let direct = &cfg.direct_domains;
    let active_force = &cfg.force_domains;
    let device_domains = &cfg.device_domain_rules;
    current = apply_domain_rules(&current, direct, active_force, device_domains)?;

    // 4. Умные гибридные правила Zapret (YouTube / Discord / GitHub / Torrents / Adult -> DIRECT, AI -> PROXY)
    // Пользовательские force_domains имеют абсолютный приоритет над DIRECT-правилами Zapret
    current = apply_zapret_hybrid_rules_with_force(&current, &cfg.zapret, active_force)?;

    // 5. Блокировка рекламы на роутере (AdBlock) — применяется ко всем устройствам
    if let Ok(with_adblock) = apply_adblock_rules(&current, cfg.adblock_enabled) {
        current = with_adblock;
    }

    // 5.1. Интеграция AdGuard Home DNS под прокси (Mihomo nameserver -> 127.0.0.1:5353)
    let agh_enabled = cfg.adguard.enabled || cfg.adblock_enabled;
    let agh_port = if cfg.adguard.dns_port > 0 { cfg.adguard.dns_port } else { 5354 };
    if let Ok(with_agh_dns) = apply_adguard_dns_upstream(&current, agh_port, agh_enabled) {
        current = with_agh_dns;
    }

    // 6. Игровой режим (приоритетный маршрут устройства и селектор-группа 🎮 Gaming)
    current = apply_gaming_rules(&current, &cfg.gaming, &providers)?;

    // 7. Выделенный маршрут Google Flow & AI (высший приоритет — на самом верху секции rules)
    let flow_target = cfg.flow_server.as_deref().filter(|s| !s.trim().is_empty()).unwrap_or("PROXY");
    let flow_group = find_flow_group_name(&current);
    if let Ok(with_flow) = apply_flow_rules(&current, flow_target.trim(), flow_group.as_deref()) {
        current = with_flow;
    }

    Ok((current, count))
}

pub fn apply_routing(yaml: &str, cfg: &crate::config::AppConfig) -> Result<(String, usize), String> {
    let mut filters = cfg.provider_filters.clone();
    apply_routing_with_filters(yaml, cfg, &mut filters)
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
        let empty_map = std::collections::BTreeMap::new();
        let out = apply_domain_rules(BASE_YAML, &["example.com".to_string()], &["forced.org".to_string()], &empty_map).unwrap();
        assert!(out.contains(DIRECT_BEGIN));
        assert!(out.contains("DOMAIN-SUFFIX,example.com,DIRECT"));
        assert!(out.contains("DOMAIN-SUFFIX,forced.org,PROXY"));
        // доменные правила — сразу после rules: (приоритет над остальными)
        let rpos = out.find("rules:").unwrap();
        let dpos = out.find("DOMAIN-SUFFIX,example.com").unwrap();
        assert!(dpos > rpos && dpos - rpos < 80);
        // повторное применение — без дублей
        let out2 = apply_domain_rules(&out, &["example.com".to_string()], &["forced.org".to_string()], &empty_map).unwrap();
        assert_eq!(out2.matches("DOMAIN-SUFFIX,example.com").count(), 1);
        // очистка — блоки удалены
        let cleared = apply_domain_rules(&out2, &[], &[], &empty_map).unwrap();
        assert!(!cleared.contains("DOMAIN-SUFFIX,example.com"));
        assert!(cleared.contains("GEOIP,RU,DIRECT"));
    }

    #[test]
    fn domain_rules_work_with_crlf() {
        let empty_map = std::collections::BTreeMap::new();
        let crlf_yaml = BASE_YAML.replace('\n', "\r\n");
        let out = apply_domain_rules(&crlf_yaml, &["example.com".to_string()], &["forced.org".to_string()], &empty_map).unwrap();
        assert!(out.contains(DIRECT_BEGIN));
        assert!(out.contains("DOMAIN-SUFFIX,example.com,DIRECT"));
        assert!(out.contains("DOMAIN-SUFFIX,forced.org,PROXY"));
        let rpos = out.find("rules:").unwrap();
        let dpos = out.find("DOMAIN-SUFFIX,example.com").unwrap();
        assert!(dpos > rpos && dpos - rpos < 80);
    }

    #[test]
    fn device_domain_rules_and_syntax() {
        let mut dev_map = std::collections::BTreeMap::new();
        dev_map.insert("192.168.2.118".to_string(), vec![
            crate::config::DeviceDomainRule { domain: "youtube.com".into(), target: "Netherlands".into() }
        ]);
        let out = apply_domain_rules(BASE_YAML, &[], &[], &dev_map).unwrap();
        assert!(out.contains(DEV_DOMAINS_BEGIN));
        assert!(out.contains("AND,((SRC-IP-CIDR,192.168.2.118/32),(DOMAIN-SUFFIX,youtube.com)),Netherlands,no-resolve"));
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
    fn test_provider_filters_are_completely_stripped_and_never_resurrected()
    {
        let pyaml = "proxy-providers:\n  geodema:\n    type: http\n    exclude-filter: \"(?i)DIRECT|Russia|RU\"\n    interval: 43200\n  geodema2:\n    type: http\nproxy-groups:\n  - name: PROXY\n    type: select\n";
        let mut saved = std::collections::BTreeMap::new();
        saved.insert("geodema".to_string(), "(?i)DIRECT|Russia|RU".to_string());
        let out = apply_ignore_to_providers(pyaml, &["Германия".to_string()], &mut saved);
        assert!(!out.contains("exclude-filter"), "proxy-providers must NEVER contain exclude-filter: OUT={out}");
        assert!(saved.is_empty(), "saved provider_filters map must be cleared to prevent resurrection");
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

    #[test]
    fn test_add_provider_to_yaml_custom_health_check() {
        let yaml = "proxy-providers:\n  old_sub:\n    type: http\n    url: \"https://old.sub/sub\"\n";
        let out = add_provider_to_yaml(
            yaml,
            "new_sub",
            "https://my.provider/sub",
            Some("http://cp.cloudflare.com/generate_204"),
            Some(600),
        ).unwrap();
        assert!(out.contains("new_sub:"));
        assert!(out.contains("url: \"http://cp.cloudflare.com/generate_204\""));
        assert!(out.contains("interval: 600"));
    }

    #[test]
    fn test_add_provider_to_yaml_with_hwid() {
        let yaml = "proxy-providers:\n  old_sub:\n    type: http\n    url: \"https://old.sub/sub\"\n";
        let out = add_provider_to_yaml_full(
            yaml,
            "geodema_test",
            "https://my.provider/sub?hwid=test-123",
            None,
            None,
            Some("test-123"),
            None,
        ).unwrap();
        assert!(out.contains("geodema_test:"));
        assert!(out.contains("x-hwid: [\"test-123\"]"));
        assert!(out.contains("User-Agent: [\"ClashMeta/1.19.24; mihomo/1.19.24\"]"));

        let hwids = parse_provider_hwids(&out);
        assert_eq!(hwids.get("geodema_test").map(String::as_str), Some("test-123"));
    }

    #[test]
    fn test_update_provider_hwid_in_yaml() {
        let yaml = r#"proxy-providers:
  geodema:
    type: http
    url: "https://my.provider/sub?token=abc"
    interval: 86400
    path: ./providers/geodema.yaml
    health-check:
      enable: true
"#;

        // 1. Добавление HWID к провайдеру без HWID
        let with_hwid = update_provider_hwid_in_yaml(yaml, "geodema", Some("my-hwid-999")).unwrap();
        assert!(with_hwid.contains("x-hwid: [\"my-hwid-999\"]"));
        let parsed_hwids = parse_provider_hwids(&with_hwid);
        assert_eq!(parsed_hwids.get("geodema").map(String::as_str), Some("my-hwid-999"));

        // 2. Обновление существующего HWID на новый
        let updated_hwid = update_provider_hwid_in_yaml(&with_hwid, "geodema", Some("new-hwid-777")).unwrap();
        assert!(updated_hwid.contains("x-hwid: [\"new-hwid-777\"]"));
        assert!(!updated_hwid.contains("my-hwid-999"));
        let parsed_hwids2 = parse_provider_hwids(&updated_hwid);
        assert_eq!(parsed_hwids2.get("geodema").map(String::as_str), Some("new-hwid-777"));

        // 3. Удаление HWID (передаём None)
        let removed_hwid = update_provider_hwid_in_yaml(&updated_hwid, "geodema", None).unwrap();
        assert!(!removed_hwid.contains("x-hwid"));
        assert!(!removed_hwid.contains("new-hwid-777"));
        let parsed_hwids3 = parse_provider_hwids(&removed_hwid);
        assert_eq!(parsed_hwids3.get("geodema"), None);

        // 4. Ошибка при несуществующем провайдере
        let err = update_provider_hwid_in_yaml(yaml, "nonexistent", Some("hwid-123"));
        assert!(err.is_err());
    }

    #[test]
    fn apply_ignore_to_groups_4_space_indentation() {
        let yaml = "proxy-groups:\n    - name: Fastest\n      type: url-test\n      proxies:\n        - Server1\n        - Server2\n";
        let ignore = vec!["Server1".to_string()];
        let out = apply_ignore_to_groups(yaml, &ignore).unwrap();
        assert!(out.contains("exclude-filter: 'Server1'"));
    }

    #[test]
    fn apply_routing_empty_config_yaml() {
        let cfg = crate::config::AppConfig::default();
        assert!(apply_routing("", &cfg).is_err());
    }

    #[test]
    fn test_apply_adblock_rules_lifecycle() {
        let yaml = "port: 7890\nrules:\n  - DOMAIN-SUFFIX,google.com,DIRECT\n  - MATCH,PROXY\n";
        // 1. Включение
        let with_adblock = apply_adblock_rules(yaml, true).unwrap();
        assert!(with_adblock.contains(ADBLOCK_BEGIN));
        assert!(with_adblock.contains(ADBLOCK_RULE));
        assert!(with_adblock.contains(ADBLOCK_END));
        assert!(with_adblock.contains("DOMAIN-SUFFIX,google.com,DIRECT"));

        // 2. Идемпотентность (повторное включение не плодит блоки)
        let dup = apply_adblock_rules(&with_adblock, true).unwrap();
        assert_eq!(dup.matches(ADBLOCK_BEGIN).count(), 1);

        // 3. Выключение
        let disabled = apply_adblock_rules(&dup, false).unwrap();
        assert!(!disabled.contains(ADBLOCK_BEGIN));
        assert!(!disabled.contains("GEOSITE,category-ads-all,REJECT"));
        assert!(disabled.contains("DOMAIN-SUFFIX,google.com,DIRECT"));
    }

    #[test]
    fn test_apply_zapret_hybrid_rules_lifecycle() {
        let yaml = "port: 7890\nrules:\n  - DOMAIN-SUFFIX,example.com,DIRECT\n  - MATCH,PROXY\n";
        let mut zapret_cfg = crate::config::ZapretConfig {
            enabled: true,
            hybrid_youtube: true,
            hybrid_discord: true,
            discord_voice_udp: true,
            youtube_turbo: false,
            general_bypass: false,
            aggressive_dpi: false,
            isolated_proxy: true,
            bypass_github: true,
            bypass_torrents: true,
            bypass_adult: true,
            custom_entries: vec![crate::config::ZapretCustomEntry {
                domain: "mysku.club".to_string(),
                enabled: true,
                cdns: vec!["img.mysku-st.ru".to_string(), "art.mysku-st.net".to_string()],
            }],
            custom_args: None,
            ..Default::default()
        };

        // 1. Включение всех гибридных правил
        let with_zapret = apply_zapret_hybrid_rules(yaml, &zapret_cfg).unwrap();
        assert!(with_zapret.contains(ZAPRET_HYBRID_BEGIN));
        assert!(with_zapret.contains("DOMAIN-SUFFIX,googlevideo.com,DIRECT"));
        assert!(with_zapret.contains("DOMAIN-SUFFIX,discord.com,DIRECT"));
        assert!(with_zapret.contains("DOMAIN-SUFFIX,raw.githubusercontent.com,DIRECT"));
        assert!(with_zapret.contains("DOMAIN-SUFFIX,rutracker.org,DIRECT"));
        assert!(with_zapret.contains("DOMAIN-SUFFIX,pornhub.com,DIRECT"));
        assert!(with_zapret.contains("DOMAIN-SUFFIX,xvideos.com,DIRECT"));
        assert!(with_zapret.contains("DOMAIN-SUFFIX,mysku.club,DIRECT"));
        assert!(with_zapret.contains("DOMAIN-SUFFIX,img.mysku-st.ru,DIRECT"));
        assert!(with_zapret.contains("DOMAIN-SUFFIX,openai.com,PROXY"));
        assert!(with_zapret.contains("DOMAIN-SUFFIX,example.com,DIRECT"));
        // DOMAIN-KEYWORD правила НЕ должны присутствовать (слишком широкие)
        assert!(!with_zapret.contains("DOMAIN-KEYWORD,youtube"));
        assert!(!with_zapret.contains("DOMAIN-KEYWORD,discord"));

        // 2. Идемпотентность
        let dup = apply_zapret_hybrid_rules(&with_zapret, &zapret_cfg).unwrap();
        assert_eq!(dup.matches(ZAPRET_HYBRID_BEGIN).count(), 1);

        // 3. Выключение YouTube (возврат в PROXY) — Discord, GitHub, Torrents, Adult, Custom и isolated_proxy остаются
        zapret_cfg.hybrid_youtube = false;
        let no_yt = apply_zapret_hybrid_rules(&dup, &zapret_cfg).unwrap();
        assert!(!no_yt.contains("googlevideo.com,DIRECT"));
        assert!(no_yt.contains("discord.com,DIRECT"));
        assert!(no_yt.contains("raw.githubusercontent.com,DIRECT"));
        assert!(no_yt.contains("pornhub.com,DIRECT"));
        assert!(no_yt.contains("mysku.club,DIRECT"));
        assert!(no_yt.contains("openai.com,PROXY"));

        // 3.1 Выключение custom entry
        zapret_cfg.custom_entries[0].enabled = false;
        let no_custom = apply_zapret_hybrid_rules(&no_yt, &zapret_cfg).unwrap();
        assert!(!no_custom.contains("mysku.club,DIRECT"));
        assert!(!no_custom.contains("img.mysku-st.ru,DIRECT"));
        assert!(no_custom.contains("raw.githubusercontent.com,DIRECT"));
        assert!(no_custom.contains("pornhub.com,DIRECT"));

        // 4. Полное отключение службы Zapret
        zapret_cfg.enabled = false;
        let disabled = apply_zapret_hybrid_rules(&no_custom, &zapret_cfg).unwrap();
        assert!(!disabled.contains(ZAPRET_HYBRID_BEGIN));
        assert!(!disabled.contains("discord.com,DIRECT"));
        assert!(!disabled.contains("raw.githubusercontent.com"));
        assert!(!disabled.contains("pornhub.com"));
        assert!(!disabled.contains("openai.com"));
        assert!(disabled.contains("DOMAIN-SUFFIX,example.com,DIRECT"));
    }

    #[test]
    fn test_zapret_hybrid_precedence_over_force_domains() {
        let yaml = "port: 7890\nproxy-groups:\n  - name: PROXY\n    type: select\nrules:\n  - MATCH,PROXY\n";
        let mut app_cfg = crate::config::AppConfig::default();
        app_cfg.force_domains = vec![
            "github.com".to_string(),
            "data.shirogames.com".to_string(),
            "pornhub.com".to_string(),
        ];
        app_cfg.zapret.enabled = true;
        app_cfg.zapret.bypass_github = true;
        app_cfg.zapret.bypass_adult = true;

        let (applied, _) = apply_routing(yaml, &app_cfg).expect("apply_routing should succeed");

        // 1. All user-forced domains must be strictly preserved in FORCE_BEGIN block
        let force_pos = applied.find(FORCE_BEGIN).expect("FORCE_BEGIN must be present");
        let force_end_pos = applied.find(FORCE_END).expect("FORCE_END must be present");
        let force_block = &applied[force_pos..force_end_pos];

        assert!(force_block.contains("DOMAIN-SUFFIX,github.com,PROXY"), "github.com must remain in FORCE block");
        assert!(force_block.contains("DOMAIN-SUFFIX,data.shirogames.com,PROXY"), "data.shirogames.com must remain in FORCE block");
        assert!(force_block.contains("DOMAIN-SUFFIX,pornhub.com,PROXY"), "pornhub.com must remain in FORCE block");

        // 2. Zapret DIRECT rules must NOT duplicate or override domains that user explicitly forced
        assert!(!applied.contains("DOMAIN-SUFFIX,github.com,DIRECT"), "github.com must not be in Zapret DIRECT when user forced it to PROXY");
        assert!(!applied.contains("DOMAIN-SUFFIX,pornhub.com,DIRECT"), "pornhub.com must not be in Zapret DIRECT when user forced it to PROXY");

        // 3. Other Zapret bypasses not forced by the user must still be active
        assert!(applied.contains(ZAPRET_HYBRID_BEGIN));
        assert!(applied.contains("DOMAIN-SUFFIX,xvideos.com,DIRECT"), "Unforced adult domains must remain in Zapret DIRECT");
    }

    #[test]
    fn test_user_force_domains_preserved_with_zapret_enabled() {
        let yaml = "port: 7890\nproxy-groups:\n  - name: PROXY\n    type: select\nrules:\n  - MATCH,PROXY\n";
        let mut app_cfg = crate::config::AppConfig::default();
        app_cfg.force_domains = vec!["youtube.com".to_string(), "discord.com".to_string()];
        app_cfg.zapret.enabled = true;
        app_cfg.zapret.hybrid_youtube = true;
        app_cfg.zapret.hybrid_discord = true;

        let (applied, _) = apply_routing(yaml, &app_cfg).expect("apply_routing should succeed");

        // User forced youtube.com and discord.com -> they must be PROXY in FORCE block
        assert!(applied.contains("DOMAIN-SUFFIX,youtube.com,PROXY"));
        assert!(applied.contains("DOMAIN-SUFFIX,discord.com,PROXY"));

        // They must NOT be in Zapret DIRECT
        assert!(!applied.contains("DOMAIN-SUFFIX,youtube.com,DIRECT"));
        assert!(!applied.contains("DOMAIN-SUFFIX,discord.com,DIRECT"));

        // But other subdomains/related domains not in force_domains must still be accelerated via Zapret DIRECT
        assert!(applied.contains("DOMAIN-SUFFIX,googlevideo.com,DIRECT"));
        assert!(applied.contains("DOMAIN-SUFFIX,discord.gg,DIRECT"));
    }

    #[test]
    fn test_is_zapret_direct_domain_exact_matching() {
        let mut zapret_cfg = crate::config::ZapretConfig::default();
        zapret_cfg.enabled = true;
        zapret_cfg.hybrid_youtube = true;
        zapret_cfg.hybrid_discord = true;
        zapret_cfg.bypass_github = true;
        zapret_cfg.bypass_torrents = true;
        zapret_cfg.bypass_adult = true;

        // Exact matches and subdomains
        assert!(is_zapret_direct_domain("github.com", &zapret_cfg));
        assert!(is_zapret_direct_domain("api.github.com", &zapret_cfg));
        assert!(is_zapret_direct_domain("raw.githubusercontent.com", &zapret_cfg));
        assert!(is_zapret_direct_domain("pornhub.com", &zapret_cfg));
        assert!(is_zapret_direct_domain("rt.pornhub.com", &zapret_cfg));
        assert!(is_zapret_direct_domain("rutracker.org", &zapret_cfg));
        assert!(is_zapret_direct_domain("googlevideo.com", &zapret_cfg));
        assert!(is_zapret_direct_domain("rr1---sn-4g5ednss.googlevideo.com", &zapret_cfg));
        assert!(is_zapret_direct_domain("discord.gg", &zapret_cfg));

        // Unrelated domains containing substring should NOT match
        assert!(!is_zapret_direct_domain("notyoutube.com", &zapret_cfg));
        assert!(!is_zapret_direct_domain("fake-discord.org", &zapret_cfg));
        assert!(!is_zapret_direct_domain("fakegithub.com", &zapret_cfg));

        // Protected Flow domains must never be treated as Zapret direct
        assert!(!is_zapret_direct_domain("googleapis.com", &zapret_cfg));
        assert!(!is_zapret_direct_domain("gemini.google.com", &zapret_cfg));
    }

    #[test]
    fn test_remove_adblock_missing_end_marker_does_not_truncate() {
        let yaml_with_missing_end = format!(
            "rules:\n{}\n  - GEOSITE,category-ads-all,REJECT\n  - GEOIP,RU,DIRECT\n  - MATCH,PROXY\n",
            ADBLOCK_BEGIN
        );
        let result = remove_adblock_rules(&yaml_with_missing_end);
        assert!(!result.contains(ADBLOCK_BEGIN));
        assert!(result.contains("GEOSITE,category-ads-all,REJECT"));
        assert!(result.contains("GEOIP,RU,DIRECT"));
        assert!(result.contains("MATCH,PROXY"));
    }

    #[test]
    fn test_remove_flow_missing_end_marker_does_not_truncate() {
        let yaml_with_missing_end = format!(
            "rules:\n{}\n  - DOMAIN-SUFFIX,google.com,FLOW\n  - GEOIP,RU,DIRECT\n",
            GOOGLE_AI_BEGIN
        );
        let result = remove_flow_rules(&yaml_with_missing_end);
        assert!(!result.contains(GOOGLE_AI_BEGIN));
        assert!(result.contains("DOMAIN-SUFFIX,google.com,FLOW"));
        assert!(result.contains("GEOIP,RU,DIRECT"));
    }

    #[test]
    fn test_remove_zapret_hybrid_missing_end_marker_does_not_truncate() {
        let yaml_with_missing_end = format!(
            "rules:\n{}\n  - DOMAIN-SUFFIX,youtube.com,DIRECT\n  - MATCH,PROXY\n",
            ZAPRET_HYBRID_BEGIN
        );
        let result = remove_zapret_hybrid_rules(&yaml_with_missing_end);
        assert!(!result.contains(ZAPRET_HYBRID_BEGIN));
        assert!(result.contains("DOMAIN-SUFFIX,youtube.com,DIRECT"));
        assert!(result.contains("MATCH,PROXY"));
    }

    #[test]
    fn test_remove_domain_blocks_missing_end_marker_does_not_truncate() {
        let yaml_with_missing_end = format!(
            "rules:\n{}\n  - DOMAIN,blocked.com,REJECT\n  - MATCH,PROXY\n",
            DIRECT_BEGIN
        );
        let result = remove_domain_blocks(&yaml_with_missing_end);
        assert!(!result.contains(DIRECT_BEGIN));
        assert!(result.contains("DOMAIN,blocked.com,REJECT"));
        assert!(result.contains("MATCH,PROXY"));
    }

    #[test]
    fn test_delete_provider_from_yaml_full_cleanup() {
        let yaml = r#"
proxy-groups:
  - name: PROXY
    type: select
    use:
      - geodema
      - subscription_1
      - geodema2
    proxies: [DIRECT, Fallback]
  - name: Discord
    type: select
    use:
      - subscription_1
    proxies: [PROXY]
  - name: Steam
    type: select
    use: [geodema, subscription_1, other]

proxy-providers:
  geodema:
    type: http
    url: "https://example.com/geo"
  subscription_1:
    type: http
    url: "https://example.com/sub1"
  geodema2:
    type: http
    url: "https://example.com/geo2"
"#;

        let cleaned = delete_provider_from_yaml(yaml, "subscription_1").unwrap();
        // Проверяем удаление из proxy-providers
        assert!(!cleaned.contains("subscription_1:"));
        assert!(cleaned.contains("geodema:"));
        assert!(cleaned.contains("geodema2:"));

        // Проверяем удаление из use: списков proxy-groups
        assert!(!cleaned.contains("- subscription_1"));
        assert!(cleaned.contains("- geodema"));
        assert!(cleaned.contains("- geodema2"));

        // Проверяем inline use: [geodema, subscription_1, other] -> [geodema, other]
        assert!(cleaned.contains("use: [geodema, other]"));
    }

    #[test]
    fn test_apply_and_remove_gaming_rules() {
        let yaml = r#"port: 7890
proxy-groups:
  - name: PROXY
    type: select
    proxies:
      - Fastest
rules:
  - GEOIP,RU,DIRECT
  - MATCH,PROXY
"#;
        let mut cfg = crate::config::GamingConfig::default();
        cfg.enabled = true;
        cfg.target_server = "LowPingNode".into();
        cfg.platforms.discord = true;
        cfg.platforms.steam = true;
        cfg.platforms.xbox = true;
        cfg.platforms.supercell = true;
        cfg.custom_domains = vec!["customgame.com".into()];

        let providers = vec!["sub1".to_string()];
        let applied = apply_gaming_rules(yaml, &cfg, &providers).expect("Must apply gaming rules");

        assert!(applied.contains(GAMING_GROUP_BEGIN));
        assert!(applied.contains(GAMING_GROUP_END));
        assert!(applied.contains("- name: '🎮 Gaming'"));
        assert!(applied.contains("- LowPingNode"));
        assert!(applied.contains("- sub1"));

        assert!(applied.contains(GAMING_BEGIN));
        assert!(applied.contains(GAMING_END));
        assert!(applied.contains("DOMAIN-SUFFIX,discord.com,🎮 Gaming"));
        assert!(applied.contains("DOMAIN-SUFFIX,steamcommunity.com,🎮 Gaming"));
        assert!(applied.contains("DOMAIN-SUFFIX,user.auth.xboxlive.com,🎮 Gaming"));
        assert!(applied.contains("DOMAIN-SUFFIX,brawlstars.com,🎮 Gaming"));
        assert!(applied.contains("DOMAIN-SUFFIX,customgame.com,🎮 Gaming"));

        // Удаление при выключении
        cfg.enabled = false;
        let disabled = apply_gaming_rules(&applied, &cfg, &providers).expect("Must disable gaming rules");
        assert!(!disabled.contains(GAMING_GROUP_BEGIN));
        assert!(!disabled.contains(GAMING_BEGIN));
        assert!(!disabled.contains("🎮 Gaming"));
        assert!(disabled.contains("MATCH,PROXY"));
    }

    #[test]
    fn test_apply_gaming_compatibility_mode_device() {
        let yaml = r#"port: 7890
proxy-groups:
  - name: PROXY
    type: select
    proxies:
      - Fastest
rules:
  - GEOIP,RU,DIRECT
  - MATCH,PROXY
"#;
        let mut cfg = crate::config::GamingConfig::default();
        cfg.enabled = true;
        cfg.mode = crate::config::GamingMode::Compatibility;
        cfg.target_server = "GameVPS".into();
        cfg.devices.push(crate::config::GamingDevice {
            mac: "00:11:22:33:44:55".into(),
            ip: "192.168.2.115".into(),
            ipv6: vec!["2001:db8::10".into()],
            name: "Gaming-Rig".into(),
            enabled: true,
            server: None,
        });

        let providers = vec![];
        let applied = apply_gaming_rules(yaml, &cfg, &providers).expect("Must apply compatibility mode");

        assert!(applied.contains(GAMING_BEGIN));
        assert!(applied.contains("SRC-IP-CIDR,192.168.2.115/32,🎮 Gaming"));
        assert!(applied.contains("SRC-IP-CIDR,2001:db8::10/128,🎮 Gaming"));
        // Не должно содержать category-games в режиме полной совместимости устройства
        assert!(!applied.contains("GEOSITE,category-games"));
    }

    #[test]
    fn test_apply_gaming_known_services_mode() {
        let yaml = r#"port: 7890
proxy-groups:
  - name: PROXY
    type: select
    proxies:
      - Fastest
rules:
  - GEOIP,RU,DIRECT
  - MATCH,PROXY
"#;
        let mut cfg = crate::config::GamingConfig::default();
        cfg.enabled = true;
        cfg.mode = crate::config::GamingMode::KnownServices;
        cfg.target_server = "Fastest".into();
        cfg.platforms.category_games = true;
        cfg.platforms.steam = true;

        let providers = vec![];
        let applied = apply_gaming_rules(yaml, &cfg, &providers).expect("Must apply known services mode");

        assert!(applied.contains(GAMING_BEGIN));
        assert!(applied.contains("GEOSITE,category-games,🎮 Gaming"));
        assert!(applied.contains("DOMAIN-SUFFIX,steamcommunity.com,🎮 Gaming"));
        assert!(!applied.contains("SRC-IP-CIDR"));
    }

    #[test]
    fn test_apply_gaming_filters_link_local_ipv6() {
        let yaml = "port: 7890\nproxy-groups:\nrules:\n  - MATCH,PROXY\n";
        let mut cfg = crate::config::GamingConfig::default();
        cfg.enabled = true;
        cfg.mode = crate::config::GamingMode::Compatibility;
        cfg.target_server = "Fastest".into();
        cfg.devices.push(crate::config::GamingDevice {
            mac: "11:22:33:44:55:66".into(),
            ip: "192.168.2.50".into(),
            ipv6: vec![
                "fe80::1ff:fe00:1".into(),
                "::1".into(),
                "2a02:1234:5678::1".into(),
            ],
            name: "Rig".into(),
            enabled: true,
            server: None,
        });

        let applied = apply_gaming_rules(yaml, &cfg, &[]).expect("applied");
        assert!(applied.contains("SRC-IP-CIDR,192.168.2.50/32,🎮 Gaming"));
        assert!(applied.contains("SRC-IP-CIDR,2a02:1234:5678::1/128,🎮 Gaming"));
        assert!(!applied.contains("fe80::1ff:fe00:1"), "fe80: must be excluded");
        assert!(!applied.contains("SRC-IP-CIDR,::1/128"), "loopback must be excluded");
    }

    #[test]
    fn test_apply_routing_gaming_priority_over_device_assignment() {
        let yaml = "port: 7890\nproxy-groups:\nrules:\n  - MATCH,PROXY\n";
        let mut app_cfg = crate::config::AppConfig::default();
        app_cfg.device_routing.insert(
            "192.168.2.115".into(),
            crate::config::DeviceRouting {
                servers: vec!["OldProxy".into()],
                ..Default::default()
            },
        );
        app_cfg.gaming.enabled = true;
        app_cfg.gaming.mode = crate::config::GamingMode::Compatibility;
        app_cfg.gaming.target_server = "GamingNode".into();
        app_cfg.gaming.devices.push(crate::config::GamingDevice {
            mac: "aa:bb:cc:dd:ee:ff".into(),
            ip: "192.168.2.115".into(),
            ipv6: vec![],
            name: "Gaming-Console".into(),
            enabled: true,
            server: None,
        });

        let (applied, _) = apply_routing(yaml, &app_cfg).expect("applied");
        let gaming_pos = applied.find(GAMING_BEGIN).expect("GAMING_BEGIN exists");
        let device_pos = applied.find(RULES_BEGIN).expect("RULES_BEGIN exists");
        assert!(
            gaming_pos < device_pos,
            "Gaming priority route must appear before general device assignment in rules"
        );
    }

    #[test]
    fn test_apply_gaming_target_server_order_in_proxies() {
        let yaml = "port: 7890\nproxy-groups:\nrules:\n  - MATCH,PROXY\n";
        let mut cfg = crate::config::GamingConfig::default();
        cfg.enabled = true;
        cfg.mode = crate::config::GamingMode::Compatibility;
        cfg.target_server = "Fastest".into();
        cfg.devices.push(crate::config::GamingDevice {
            mac: "11:22:33:44:55:66".into(),
            ip: "192.168.2.50".into(),
            ipv6: vec![],
            name: "PC".into(),
            enabled: true,
            server: None,
        });

        let applied = apply_gaming_rules(yaml, &cfg, &[]).expect("applied");
        // Fastest must be the first proxy listed under proxies: in the 🎮 Gaming group
        let group_block = extract_block(&applied, GAMING_GROUP_BEGIN, GAMING_GROUP_END).expect("gaming group block");
        let proxies_pos = group_block.find("proxies:").expect("proxies section");
        let after_proxies = &group_block[proxies_pos..];
        let first_proxy = after_proxies.lines().nth(1).expect("first proxy line").trim();
        assert_eq!(first_proxy, "- Fastest", "Fastest must be the first proxy in the select group, not DIRECT");
    }

    #[test]
    fn test_apply_gaming_excludes_disabled_devices_when_multiple() {
        let yaml = "port: 7890\nproxy-groups:\nrules:\n  - MATCH,PROXY\n";
        let mut cfg = crate::config::GamingConfig::default();
        cfg.enabled = true;
        cfg.mode = crate::config::GamingMode::Compatibility;
        cfg.target_server = "Fastest".into();
        cfg.devices.push(crate::config::GamingDevice {
            mac: "11:11:11:11:11:11".into(),
            ip: "192.168.2.10".into(),
            ipv6: vec![],
            name: "Active-Console".into(),
            enabled: true,
            server: None,
        });
        cfg.devices.push(crate::config::GamingDevice {
            mac: "22:22:22:22:22:22".into(),
            ip: "192.168.2.20".into(),
            ipv6: vec![],
            name: "Inactive-PC".into(),
            enabled: false,
            server: None,
        });

        let applied = apply_gaming_rules(yaml, &cfg, &[]).expect("applied");
        assert!(applied.contains("SRC-IP-CIDR,192.168.2.10/32,🎮 Gaming"));
        assert!(!applied.contains("SRC-IP-CIDR,192.168.2.20/32,🎮 Gaming"), "Disabled device must not be routed");
    }

    #[test]
    fn test_format_src_ip_cidr_no_duplicate_suffix() {
        assert_eq!(format_src_ip_cidr("192.168.1.1"), "192.168.1.1/32");
        assert_eq!(format_src_ip_cidr("192.168.1.1/32"), "192.168.1.1/32");
        assert_eq!(format_src_ip_cidr("10.0.0.0/24"), "10.0.0.0/24");
        assert_eq!(format_src_ip_cidr("2001:db8::1"), "2001:db8::1/128");
        assert_eq!(format_src_ip_cidr("2001:db8::1/128"), "2001:db8::1/128");
        assert_eq!(rule_line("192.168.1.1/32", "DEV_GROUP"), "  - SRC-IP-CIDR,192.168.1.1/32,DEV_GROUP");
    }

    #[test]
    fn test_apply_domain_rules_deduplicates_direct_and_force() {
        let yaml = "rules:\n  - MATCH,PROXY\n";
        let direct = vec!["conflict.com".to_string(), "only-direct.com".to_string()];
        let force = vec!["conflict.com".to_string(), "only-force.com".to_string()];
        let empty_dev = std::collections::BTreeMap::new();
        let res = apply_domain_rules(yaml, &direct, &force, &empty_dev).expect("success");
        assert!(res.contains("DOMAIN-SUFFIX,conflict.com,DIRECT"));
        assert!(!res.contains("DOMAIN-SUFFIX,conflict.com,PROXY"), "Conflicting domain must not be added to PROXY when present in DIRECT");
        assert!(res.contains("DOMAIN-SUFFIX,only-force.com,PROXY"));
    }

    #[test]
    fn test_apply_gaming_multiple_devices_with_custom_servers() {
        let yaml = "port: 7890\nproxy-groups:\nrules:\n  - MATCH,PROXY\n";
        let mut cfg = crate::config::GamingConfig::default();
        cfg.enabled = true;
        cfg.mode = crate::config::GamingMode::Compatibility;
        cfg.target_server = "Fastest".into();
        cfg.devices.push(crate::config::GamingDevice {
            mac: "11:11:11:11:11:11".into(),
            ip: "192.168.2.10".into(),
            ipv6: vec![],
            name: "PC-Gamer".into(),
            enabled: true,
            server: Some("Finland-Node".into()),
        });
        cfg.devices.push(crate::config::GamingDevice {
            mac: "22:22:22:22:22:22".into(),
            ip: "192.168.2.15".into(),
            ipv6: vec![],
            name: "PlayStation-5".into(),
            enabled: true,
            server: None, // Uses default gaming group
        });
        cfg.devices.push(crate::config::GamingDevice {
            mac: "33:33:33:33:33:33".into(),
            ip: "192.168.2.25".into(),
            ipv6: vec![],
            name: "Nintendo-Switch".into(),
            enabled: true,
            server: Some("Poland-Node".into()),
        });

        let applied = apply_gaming_rules(yaml, &cfg, &[]).expect("applied");
        assert!(applied.contains("SRC-IP-CIDR,192.168.2.10/32,Finland-Node"), "PC must route to custom Finland node");
        assert!(applied.contains("SRC-IP-CIDR,192.168.2.15/32,🎮 Gaming"), "PS5 must route to default Gaming group");
        assert!(applied.contains("SRC-IP-CIDR,192.168.2.25/32,Poland-Node"), "Switch must route to custom Poland node");
    }

    #[test]
    fn test_apply_gaming_bypass_ru_preset() {
        let yaml = "port: 7890\nproxy-groups:\nrules:\n  - MATCH,PROXY\n";
        let mut cfg = crate::config::GamingConfig::default();
        cfg.enabled = true;
        cfg.mode = crate::config::GamingMode::BypassRu;
        cfg.target_server = "Fastest".into();
        cfg.devices.push(crate::config::GamingDevice {
            mac: "11:11:11:11:11:11".into(),
            ip: "192.168.2.10".into(),
            ipv6: vec![],
            name: "PC-Gamer".into(),
            enabled: true,
            server: Some("Finland-Node".into()),
        });

        let applied = apply_gaming_rules(yaml, &cfg, &[]).expect("applied");
        assert!(applied.contains("AND,((SRC-IP-CIDR,192.168.2.10/32),(GEOIP,RU)),DIRECT"), "RU GeoIP must be direct in BypassRu");
        assert!(applied.contains("AND,((SRC-IP-CIDR,192.168.2.10/32),(GEOSITE,category-ru)),DIRECT"), "RU Geosite must be category-ru");
        assert!(!applied.contains("(GEOSITE,ru))"), "Must never generate invalid (GEOSITE,ru)");
        assert!(applied.contains("SRC-IP-CIDR,192.168.2.10/32,Finland-Node"), "Remaining traffic must route to Finland-Node");
    }

    #[test]
    fn test_apply_gaming_bypass_ru_with_ruleset() {
        let yaml = "port: 7890\nrule-providers:\n  category-ru@domain: {}\n  category-ru@ipcidr: {}\nproxy-groups:\nrules:\n  - MATCH,PROXY\n";
        let mut cfg = crate::config::GamingConfig::default();
        cfg.enabled = true;
        cfg.mode = crate::config::GamingMode::BypassRu;
        cfg.target_server = "Fastest".into();
        cfg.devices.push(crate::config::GamingDevice {
            mac: "11:11:11:11:11:11".into(),
            ip: "192.168.2.10".into(),
            ipv6: vec![],
            name: "PC-Gamer".into(),
            enabled: true,
            server: Some("Finland-Node".into()),
        });

        let applied = apply_gaming_rules(yaml, &cfg, &[]).expect("applied");
        assert!(applied.contains("AND,((SRC-IP-CIDR,192.168.2.10/32),(GEOIP,RU)),DIRECT"));
        assert!(applied.contains("AND,((SRC-IP-CIDR,192.168.2.10/32),(RULE-SET,category-ru@domain)),DIRECT"));
        assert!(applied.contains("AND,((SRC-IP-CIDR,192.168.2.10/32),(RULE-SET,category-ru@ipcidr)),DIRECT"));
        assert!(!applied.contains("GEOSITE,ru"), "Must not use GEOSITE when RULE-SET is available");
        assert!(applied.contains("SRC-IP-CIDR,192.168.2.10/32,Finland-Node"));
    }

    #[test]
    fn test_parse_redir_port() {
        let yaml1 = "redir-port: 5000\nport: 7890\n";
        assert_eq!(parse_redir_port(yaml1), Some(5000));

        let yaml2 = "redir-port: '5200'\n";
        assert_eq!(parse_redir_port(yaml2), Some(5200));

        let yaml3 = "tproxy-port: 5000\n";
        assert_eq!(parse_redir_port(yaml3), None);
    }

    #[test]
    fn test_parse_tproxy_port() {
        let yaml1 = "tproxy-port: 5001\nport: 7890\n";
        assert_eq!(parse_tproxy_port(yaml1), Some(5001));

        let yaml2 = "tproxy-port: '5005'\n";
        assert_eq!(parse_tproxy_port(yaml2), Some(5005));

        let yaml3 = "redir-port: 5000\n";
        assert_eq!(parse_tproxy_port(yaml3), None);
    }

    #[test]
    fn test_provider_and_flow_char_validation() {
        // SEC-12: Reject injection chars in add_provider_to_yaml_full and apply_flow_rules
        let yaml = "proxy-providers:\n  sub1:\n    type: http\n    url: \"https://foo.bar/sub\"\nrules:\n  - MATCH,DIRECT";
        assert!(add_provider_to_yaml_full(yaml, "bad\nid", "https://foo.bar/sub", None, None, None, None).is_err());
        assert!(add_provider_to_yaml_full(yaml, "sub2", "https://foo.bar/sub\"evil", None, None, None, None).is_err());
        assert!(add_provider_to_yaml_full(yaml, "sub2", "https://foo.bar/sub\nevil", None, None, None, None).is_err());
        assert!(add_provider_to_yaml_full(yaml, "sub2", "https://foo.bar/sub", None, None, Some("hwid\"inject"), None).is_err());
        assert!(add_provider_to_yaml_full(yaml, "sub2", "https://foo.bar/sub", None, None, None, Some("ua\ninject")).is_err());

        assert!(apply_flow_rules(yaml, "evil.com\"injected", Some("PROXY")).is_err());
        assert!(apply_flow_rules(yaml, "evil.com\ninjected", Some("PROXY")).is_err());
        assert!(apply_flow_rules(yaml, "evil.com", Some("PROXY\"injected")).is_err());
    }

    #[test]
    fn test_apply_domain_rules_dynamic_group() {
        // BUG-17: Dynamic group resolution in apply_domain_rules
        let yaml = "proxy-groups:\n  - name: MY_CUSTOM_PROXY\n    type: select\nrules:\n  - MATCH,DIRECT";
        let empty_device_domains = std::collections::BTreeMap::new();
        let out = apply_domain_rules(yaml, &[], &["customforce.com".to_string()], &empty_device_domains).unwrap();
        assert!(out.contains("DOMAIN-SUFFIX,customforce.com,MY_CUSTOM_PROXY"));
        assert!(!out.contains("DOMAIN-SUFFIX,customforce.com,PROXY"));
    }

    #[test]
    fn test_bug18_rule_hierarchy_adblock_and_direct_precede_device_assignment() {
        // BUG-18: Rules hierarchy must be FIFO: AdBlock, Direct, Zapret hybrid must precede device rules
        let yaml = "port: 7890\nproxy-groups:\nrules:\n  - MATCH,PROXY\n";
        let mut app_cfg = crate::config::AppConfig::default();
        app_cfg.adblock_enabled = true;
        app_cfg.direct_domains = vec!["mysite.direct.ru".to_string()];
        app_cfg.zapret.enabled = true;
        app_cfg.zapret.hybrid_youtube = true;
        app_cfg.device_routing.insert(
            "192.168.1.55".into(),
            crate::config::DeviceRouting {
                servers: vec!["AssignedNode".into()],
                ..Default::default()
            },
        );

        let (applied, _) = apply_routing(yaml, &app_cfg).expect("apply_routing should succeed");
        let adblock_pos = applied.find(ADBLOCK_BEGIN).expect("ADBLOCK_BEGIN must exist");
        let direct_pos = applied.find(DIRECT_BEGIN).expect("DIRECT_BEGIN must exist");
        let zapret_pos = applied.find(ZAPRET_HYBRID_BEGIN).expect("ZAPRET_HYBRID_BEGIN must exist");
        let device_pos = applied.find(RULES_BEGIN).expect("RULES_BEGIN must exist");

        assert!(adblock_pos < device_pos, "AdBlock must be placed before device assignments so ads are blocked");
        assert!(direct_pos < device_pos, "Direct domains must be placed before device assignments so direct traffic is not proxied");
        assert!(zapret_pos < device_pos, "Zapret hybrid rules must be placed before device assignments");
    }

    #[test]
    fn test_bug20_parse_and_preserve_nonstandard_cidr_masks() {
        // BUG-20: Correct parsing, group mapping and formatting for non-/32 and non-/128 CIDRs
        let yaml = "rules:\n# --- AUTO-DEVICE-RULES-BEGIN ---\n  - SRC-IP-CIDR,192.168.10.0/24,OfficeSubnet\n  - SRC-IP-CIDR,10.0.0.0/8,VpnSubnet\n  - SRC-IP-CIDR,2001:db8::/64,Ipv6Subnet\n# --- AUTO-DEVICE-RULES-END ---\n";
        let parsed = parse_rules(yaml);
        assert!(parsed.contains_key("192.168.10.0/24"), "Must parse /24 mask");
        assert!(parsed.contains_key("10.0.0.0/8"), "Must parse /8 mask");
        assert!(parsed.contains_key("2001:db8::/64"), "Must parse /64 mask");

        // Group name encoding & decoding roundtrip
        let gname = group_name_for("192.168.10.0/24", "Office Network");
        assert_eq!(gname, "Office Network 192_168_10_0_24");
        let decoded = ip_key_from_group(&gname);
        assert_eq!(decoded, Some("192.168.10.0/24".to_string()));

        // Formatting
        assert_eq!(format_src_ip_cidr("192.168.10.0/24"), "192.168.10.0/24");
        assert_eq!(format_src_ip_cidr("10.0.0.0/8"), "10.0.0.0/8");
        assert_eq!(format_src_ip_cidr("2001:db8::/64"), "2001:db8::/64");
    }

    #[test]
    fn test_bug07_provider_yaml_formatting_and_crlf_lf_preservation() {
        // BUG-07: Consistent 2-space indentation and CRLF/LF preservation
        let lf_yaml = "proxy-providers:\n  sub1:\n    type: http\n    url: \"https://a.b/c\"\nrules:\n  - MATCH,DIRECT\n";
        let out_lf = add_provider_to_yaml(lf_yaml, "sub2", "https://x.y/z", None, None).expect("added");
        assert!(!out_lf.contains("\r\n"), "LF input must preserve LF");
        assert!(out_lf.contains("  sub2:\n    type: http\n    url: \"https://x.y/z\""), "Must use 2-space indentation hierarchy");

        let crlf_yaml = "proxy-providers:\r\n  sub1:\r\n    type: http\r\n    url: \"https://a.b/c\"\r\nrules:\r\n  - MATCH,DIRECT\r\n";
        let out_crlf = add_provider_to_yaml(crlf_yaml, "sub2", "https://x.y/z", None, None).expect("added");
        assert!(out_crlf.contains("\r\n"), "CRLF input must preserve CRLF");
        assert!(out_crlf.contains("  sub2:\r\n    type: http\r\n    url: \"https://x.y/z\""));

        let deleted = delete_provider_from_yaml(&out_crlf, "sub2").expect("deleted");
        assert!(deleted.contains("\r\n"), "CRLF deletion must preserve CRLF");
        assert!(!deleted.contains("sub2:"));
    }

    #[test]
    fn test_anti_regression_exclude_filter_completely_removed_from_providers_in_apply_routing() {
        // Anti-regression: Exclude-filter in proxy-providers must be stripped to preserve all 13 servers
        let yaml = "proxy-providers:\n  myprov:\n    type: http\n    url: \"https://a.b/c\"\n    exclude-filter: '(?i)DIRECT|Russia|RU'\nproxy-groups:\n  - name: Fastest\n    type: url-test\n    use:\n      - myprov\nrules:\n  - MATCH,DIRECT\n";
        let mut app_cfg = crate::config::AppConfig::default();
        app_cfg.ignore_servers = vec!["BadNode".to_string()];
        let mut filters = std::collections::BTreeMap::new();
        filters.insert("myprov".to_string(), "(?i)DIRECT|Russia|RU".to_string());
        let (out, _) = apply_routing_with_filters(yaml, &app_cfg, &mut filters).expect("applied");
        assert!(!out.contains("exclude-filter: '(?i)DIRECT|Russia|RU'"), "Provider exclude-filter must be completely stripped");
        assert!(filters.is_empty(), "Filters map must be cleared to prevent resurrection");
    }

    #[test]
    fn test_parse_provider_urls_does_not_get_overwritten_by_health_check() {
        let yaml = r#"
proxy-providers:
  geodema2:
    type: http
    url: "https://my.subscription.org/sub/token123"
    interval: 43200
    health-check:
      enable: true
      url: "https://www.gstatic.com/generate_204"
      interval: 300
"#;
        let map = parse_provider_urls(yaml);
        assert_eq!(map.get("geodema2"), Some(&"https://my.subscription.org/sub/token123".to_string()));
        assert_ne!(map.get("geodema2"), Some(&"https://www.gstatic.com/generate_204".to_string()));
    }

    #[test]
    fn test_convert_http_provider_to_file_in_yaml() {
        let yaml = r#"
proxy-providers:
  geodema2:
    type: http
    url: "https://my.subscription.org/sub/token123"
    interval: 43200
    exclude-filter: "(?i)DIRECT|Russia|RU"
    health-check:
      enable: true
      url: "https://www.gstatic.com/generate_204"
      interval: 300
"#;
        let converted = convert_http_provider_to_file_in_yaml(yaml, "geodema2", "./providers/geodema2.yaml").expect("converted");
        assert!(converted.contains("    type: file"), "Must be type: file");
        assert!(converted.contains("    path: \"./providers/geodema2.yaml\""), "Must contain path");
        assert!(!converted.contains("    url: \"https://my.subscription.org"), "Provider-level url must be removed");
        assert!(!converted.contains("    interval: 43200"), "Provider-level interval must be removed");
        assert!(!converted.contains("exclude-filter"), "exclude-filter must be stripped");
        assert!(converted.contains("health-check:"), "health-check must be preserved");
        assert!(converted.contains("url: \"https://www.gstatic.com/generate_204\""), "health-check url must be preserved");
    }

    #[test]
    fn test_apply_adguard_dns_upstream_lifecycle() {
        let sample_yaml = r#"
dns:
  enable: true
  listen: 0.0.0.0:1053
  nameserver:
    - 77.88.8.8
    - 1.1.1.1
  direct-nameserver:
    - 77.88.8.8
rules:
  - MATCH,DIRECT
"#;
        // 1. Включение AdGuard Home (порт 5353)
        let applied = apply_adguard_dns_upstream(sample_yaml, 5353, true).expect("must succeed");
        assert!(applied.contains(AGH_DNS_BEGIN));
        assert!(applied.contains("- 127.0.0.1:5353"));
        assert!(applied.contains(AGH_DNS_END));
        assert!(applied.contains(AGH_DIRECT_DNS_BEGIN));
        assert!(applied.contains(AGH_DIRECT_DNS_END));
        assert!(applied.contains("- 77.88.8.8"), "Existing nameservers must be preserved as fallbacks");

        // 2. Идемпотентность (повторный вызов не дублирует блоки)
        let re_applied = apply_adguard_dns_upstream(&applied, 5353, true).expect("must succeed");
        assert_eq!(re_applied.matches(AGH_DNS_BEGIN).count(), 1, "Must contain exactly one AGH_DNS_BEGIN");
        assert_eq!(re_applied.matches("127.0.0.1:5353").count(), 2, "Must contain 127.0.0.1:5353 once for nameserver and once for direct-nameserver");

        // 3. Отключение AdGuard Home — чистое удаление
        let disabled = apply_adguard_dns_upstream(&re_applied, 5353, false).expect("must succeed");
        assert!(!disabled.contains(AGH_DNS_BEGIN));
        assert!(!disabled.contains("127.0.0.1:5353"));
        assert!(!disabled.contains(AGH_DNS_END));
        assert!(disabled.contains("- 77.88.8.8"), "Original resolvers must remain intact");
    }

    #[test]
    fn test_domestic_catalog_invariants() {
        assert_eq!(DOMESTIC_WHITELIST_DOMAINS.len(), 402, "Must contain exactly 402 base domains");
        assert_eq!(DOMESTIC_CATALOG.len(), 402, "Catalog entries must equal 402");

        let mut seen = std::collections::HashSet::new();
        for &dom in DOMESTIC_WHITELIST_DOMAINS {
            assert!(seen.insert(dom), "Duplicate domain found in catalog: {}", dom);
            assert!(crate::config::is_valid_domain(dom), "Domain failed validity check: {}", dom);
            assert!(!dom.contains("://"), "Domain must not contain protocol: {}", dom);
            assert!(!dom.contains('/'), "Domain must not contain path: {}", dom);
            assert!(!dom.starts_with("www."), "Domain must not contain www prefix: {}", dom);
        }
    }

    #[test]
    fn test_apply_whitelist_rules_lifecycle() {
        let base_yaml = "port: 7890\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies:\n      - Fastest\nrules:\n  - GEOIP,RU,DIRECT\n  - MATCH,PROXY\n";
        let mut cfg = crate::config::AppConfig::default();
        cfg.whitelist.enabled = true;
        cfg.whitelist.devices.push(crate::config::WhitelistDevice {
            mac: "AA:BB:CC:DD:EE:01".into(),
            ip: "192.168.1.105".into(),
            enabled: true,
            name: Some("Work Laptop".into()),
            server: None
        });

        // 1. Применение правил
        let applied = apply_whitelist_rules(base_yaml, &cfg, None);
        assert!(applied.contains(WHITELIST_BEGIN));
        assert!(applied.contains(WHITELIST_END));
        assert!(applied.contains(WHITELIST_GROUP_BEGIN));
        assert!(applied.contains(WHITELIST_GROUP_END));
        assert!(applied.contains(&format!("- name: '{WHITELIST_GROUP_NAME}'")));
        assert!(applied.contains("DOMAIN-SUFFIX,gosuslugi.ru,DIRECT"));
        assert!(applied.contains("DOMAIN-SUFFIX,sberbank.ru,DIRECT"));
        assert!(applied.contains("DOMAIN-SUFFIX,vk.com,DIRECT"));
        assert!(applied.contains(&format!("SRC-IP-CIDR,192.168.1.105/32,{WHITELIST_GROUP_NAME}")));
        assert!(validate_marker_pair(&applied, WHITELIST_BEGIN, WHITELIST_END).is_ok());
        assert!(validate_marker_pair(&applied, WHITELIST_GROUP_BEGIN, WHITELIST_GROUP_END).is_ok());

        // 2. Отключение — чистое удаление
        cfg.whitelist.enabled = false;
        let disabled = apply_whitelist_rules(&applied, &cfg, None);
        assert!(!disabled.contains(WHITELIST_BEGIN));
        assert!(!disabled.contains(WHITELIST_END));
        assert!(!disabled.contains(WHITELIST_GROUP_BEGIN));
        assert!(!disabled.contains(WHITELIST_GROUP_END));
        assert!(!disabled.contains("SRC-IP-CIDR,192.168.1.105/32"));
        assert!(disabled.contains("GEOIP,RU,DIRECT"), "Original fallback rules must be preserved");
    }

    #[test]
    fn test_whitelist_strict_direct_no_geoip_ru_wildcard() {
        let base_yaml = "port: 7890\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies:\n      - Fastest\nrules:\n  - GEOIP,RU,DIRECT\n  - MATCH,PROXY\n";
        let mut cfg = crate::config::AppConfig::default();
        cfg.whitelist.enabled = true;
        cfg.whitelist.devices.push(crate::config::WhitelistDevice {
            mac: "AA:BB:CC:DD:EE:01".into(),
            ip: "192.168.1.105".into(),
            enabled: true,
            name: Some("Work Laptop".into()),
            server: None,
        });

        let applied = apply_whitelist_rules(base_yaml, &cfg, None);
        let start = applied.find(WHITELIST_BEGIN).expect("must find begin");
        let end = applied.find(WHITELIST_END).expect("must find end");
        let wl_block = &applied[start..end];

        // Проверяем, что внутри блока белых списков НЕТ правила GEOIP,RU
        assert!(
            !wl_block.contains("GEOIP,RU"),
            "CRITICAL REGRESSION: Whitelist block must NEVER contain broad GEOIP,RU wildcard!"
        );
        assert!(
            !wl_block.contains("GEOIP"),
            "Whitelist block must strictly use explicit DOMAIN-SUFFIX and SRC-IP-CIDR rules!"
        );
    }

    /// Анти-регрессия BS-PIN: устройство с закреплённой нодой получает СВОЁ правило
    /// SRC-IP-CIDR с этой нодой, и оно стоит ВЫШЕ подсетевого catch-all 2.4 →
    /// Failover-переключения группы Whitelist не влияют на трафик устройства.
    #[test]
    fn test_whitelist_device_pinned_server_rule_precedes_catchall() {
        let base_yaml = "port: 7890\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies:\n      - Fastest\nrules:\n  - GEOIP,RU,DIRECT\n  - MATCH,PROXY\n";
        let mut cfg = crate::config::AppConfig::default();
        cfg.whitelist.enabled = true;
        cfg.whitelist.devices.push(crate::config::WhitelistDevice {
            mac: "AA:BB:CC:DD:EE:02".into(),
            ip: "192.168.2.63".into(),
            enabled: true,
            name: Some("Xiaomi 11T Pro".into()),
            server: Some("🇳🇱 Нидерланды ADS".into()),
        });
        // Второе устройство без закрепления — обычный dev_target
        cfg.whitelist.devices.push(crate::config::WhitelistDevice {
            mac: "AA:BB:CC:DD:EE:03".into(),
            ip: "192.168.2.118".into(),
            enabled: true,
            name: Some("Big PC".into()),
            server: None,
        });

        let applied = apply_whitelist_rules(base_yaml, &cfg, None);
        let start = applied.find(WHITELIST_BEGIN).expect("must find begin");
        let end = applied.find(WHITELIST_END).expect("must find end");
        let wl_block = &applied[start..end];

        let pinned_rule = "SRC-IP-CIDR,192.168.2.63/32,🇳🇱 Нидерланды ADS";
        let plain_rule = "SRC-IP-CIDR,192.168.2.118/32";
        let catchall = "SRC-IP-CIDR,192.168.0.0/16";

        assert!(wl_block.contains(pinned_rule), "правило закрепления устройства должно появиться");
        assert!(wl_block.contains(plain_rule), "устройство без закрепления идёт на dev_target");
        assert!(wl_block.contains(catchall), "catch-all подсети обязан остаться");

        // Закреплённое правило и правило обычного устройства стоят ВЫШЕ catch-all
        let pos_pinned = wl_block.find(pinned_rule).unwrap();
        let pos_plain = wl_block.find(plain_rule).unwrap();
        let pos_catchall = wl_block.find(catchall).unwrap();
        assert!(pos_pinned < pos_catchall, "BS-PIN правило должно стоять выше catch-all");
        assert!(pos_plain < pos_catchall, "обычное правило устройства должно стоять выше catch-all");
    }

    /// BS-PIN: пустое имя ноды трактуется как «не закреплено» (dev_target), а не как битое правило.
    #[test]
    fn test_whitelist_device_empty_pinned_server_falls_back_to_target() {
        let base_yaml = "port: 7890\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies:\n      - Fastest\nrules:\n  - MATCH,PROXY\n";
        let mut cfg = crate::config::AppConfig::default();
        cfg.whitelist.enabled = true;
        cfg.whitelist.devices.push(crate::config::WhitelistDevice {
            mac: "AA:BB:CC:DD:EE:04".into(),
            ip: "192.168.2.63".into(),
            enabled: true,
            name: None,
            server: Some("   ".into()),
        });

        let applied = apply_whitelist_rules(base_yaml, &cfg, None);
        assert!(
            applied.contains("SRC-IP-CIDR,192.168.2.63/32,🛡️ Whitelist"),
            "пустое закрепление должно дать правило с группой Whitelist, а не пустым таргетом"
        );
        assert!(
            !applied.contains("SRC-IP-CIDR,192.168.2.63/32, \n"),
            "пробельное имя ноды не должно попасть в правило"
        );
    }

    #[test]
    fn test_whitelist_rule_hierarchy_precedes_device_assignments() {
        let base_yaml = "port: 7890\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies:\n      - Fastest\nrules:\n  - GEOIP,RU,DIRECT\n  - MATCH,PROXY\n";
        let mut cfg = crate::config::AppConfig::default();
        cfg.device_routing.insert("192.168.1.105".into(), crate::config::DeviceRouting {
            servers: vec!["Fastest".into()],
            ..Default::default()
        });
        cfg.whitelist.enabled = true;
        cfg.whitelist.devices.push(crate::config::WhitelistDevice {
            mac: "AA:BB:CC:DD:EE:01".into(),
            ip: "192.168.1.105".into(),
            enabled: true,
            name: Some("Work Laptop".into()),
            server: None
        });

        let (applied, _) = apply_routing(base_yaml, &cfg).expect("apply_routing must succeed");
        let wl_pos = applied.find(WHITELIST_BEGIN).expect("WHITELIST_BEGIN must be present");
        let dev_pos = applied.find(RULES_BEGIN).expect("RULES_BEGIN must be present");

        // AUTO-WHITELIST обязан быть ВЫШЕ AUTO-DEVICE-RULES в секции rules:
        assert!(
            wl_pos < dev_pos,
            "CRITICAL REGRESSION: Whitelist rules ({}) must precede general device rules ({}) to avoid routing bypass",
            wl_pos, dev_pos
        );
    }

    #[test]
    fn test_whitelist_nameserver_policy_injection() {
        let base_yaml = "port: 7890\ndns:\n  enable: true\n  nameserver:\n    - 77.88.8.8\nrules:\n  - MATCH,DIRECT\n";
        let mut cfg = crate::config::AppConfig::default();
        cfg.whitelist.enabled = true;
        cfg.whitelist.devices.push(crate::config::WhitelistDevice {
            mac: "AA:BB:CC:DD:EE:01".into(),
            ip: "192.168.1.105".into(),
            enabled: true,
            name: Some("Phone".into()),
            server: None
        });

        let applied = apply_whitelist_dns(base_yaml, &cfg, "127.0.0.1:53");
        assert!(applied.contains(WHITELIST_DNS_BEGIN));
        assert!(applied.contains(WHITELIST_DNS_END));
        assert!(applied.contains("nameserver-policy:"));
        assert!(applied.contains("'+.gosuslugi.ru': '127.0.0.1:53'"));
        assert!(applied.contains("'+.sberbank.ru': '127.0.0.1:53'"));
        assert!(applied.contains("- 77.88.8.8"), "Existing nameservers must be preserved");

        // Отключение очищает nameserver-policy
        cfg.whitelist.enabled = false;
        let disabled = apply_whitelist_dns(&applied, &cfg, "127.0.0.1:53");
        assert!(!disabled.contains(WHITELIST_DNS_BEGIN));
        assert!(!disabled.contains(WHITELIST_DNS_END));
        assert!(!disabled.contains("'+.gosuslugi.ru'"));
        assert!(disabled.contains("- 77.88.8.8"));
    }

    #[test]
    fn test_whitelist_marker_pair_validation() {
        let valid_yaml = format!("rules:\n{}\n  - DOMAIN-SUFFIX,gosuslugi.ru,DIRECT\n{}\n", WHITELIST_BEGIN, WHITELIST_END);
        assert!(validate_marker_pair(&valid_yaml, WHITELIST_BEGIN, WHITELIST_END).is_ok());

        let missing_end = format!("rules:\n{}\n  - DOMAIN-SUFFIX,gosuslugi.ru,DIRECT\n", WHITELIST_BEGIN);
        assert!(validate_marker_pair(&missing_end, WHITELIST_BEGIN, WHITELIST_END).is_err());

        let duplicate_begin = format!("rules:\n{}\n{}\n{}\n", WHITELIST_BEGIN, WHITELIST_BEGIN, WHITELIST_END);
        assert!(validate_marker_pair(&duplicate_begin, WHITELIST_BEGIN, WHITELIST_END).is_err());
    }

    #[test]
    fn test_whitelist_custom_domains() {
        let mut wl_cfg = crate::config::WhitelistConfig::default();
        wl_cfg.custom_domains = vec![
            "https://my-internal-portal.ru/index.html".into(),
            "www.special-bank.ru".into(),
            "invalid domain name with spaces".into(),
            "192.168.1.1".into(),
        ];

        let all = get_all_whitelist_domains(&wl_cfg);
        assert_eq!(all.len(), DOMESTIC_WHITELIST_DOMAINS.len() + 2, "Must add exactly 2 valid custom domains");
        assert!(all.contains(&"my-internal-portal.ru".to_string()));
        assert!(all.contains(&"special-bank.ru".to_string()));
        assert!(!all.contains(&"invalid domain name with spaces".to_string()));
        assert!(!all.contains(&"192.168.1.1".to_string()));
    }

    #[test]
    fn test_zero_touch_whitelist_subnet_rules_and_exclusions() {
        let base_yaml = "port: 7890\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies:\n      - Fastest\nrules:\n  - MATCH,PROXY\n";
        let mut cfg = crate::config::AppConfig::default();
        cfg.whitelist.enabled = true;
        cfg.whitelist.strategy = crate::config::WhitelistStrategy::StrictXKeen;
        cfg.whitelist.excluded_devices = vec!["192.168.1.99".into()];

        let applied = apply_whitelist_rules(base_yaml, &cfg, None);
        assert!(applied.contains(WHITELIST_BEGIN));
        assert!(applied.contains(WHITELIST_END));

        // 1. Domestic domains -> DIRECT
        assert!(applied.contains("DOMAIN-SUFFIX,gosuslugi.ru,DIRECT"));
        assert!(applied.contains("DOMAIN-SUFFIX,sberbank.ru,DIRECT"));

        // 2. Excluded device -> DIRECT
        assert!(applied.contains("SRC-IP-CIDR,192.168.1.99/32,DIRECT"));

        // 3. Zero-touch subnet coverage
        assert!(applied.contains("SRC-IP-CIDR,192.168.0.0/16,🛡️ Whitelist"));
        assert!(applied.contains("SRC-IP-CIDR,10.0.0.0/8,🛡️ Whitelist"));

        // 4. Hierarchy inside whitelist block: domain DIRECT must be ABOVE subnet tunnel
        let dom_pos = applied.find("DOMAIN-SUFFIX,gosuslugi.ru,DIRECT").unwrap();
        let ex_pos = applied.find("SRC-IP-CIDR,192.168.1.99/32,DIRECT").unwrap();
        let sub_pos = applied.find("SRC-IP-CIDR,192.168.0.0/16,🛡️ Whitelist").unwrap();
        assert!(dom_pos < sub_pos, "Domestic DIRECT rules must precede subnet redirection");
        assert!(ex_pos < sub_pos, "Excluded devices must precede subnet redirection");
    }

    #[test]
    fn test_standard_whitelist_strategy_no_subnet_redirection() {
        let base_yaml = "port: 7890\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies:\n      - Fastest\nrules:\n  - MATCH,PROXY\n";
        let mut cfg = crate::config::AppConfig::default();
        cfg.whitelist.enabled = true;
        cfg.whitelist.strategy = crate::config::WhitelistStrategy::Standard;

        let applied = apply_whitelist_rules(base_yaml, &cfg, None);
        assert!(applied.contains(WHITELIST_BEGIN));
        assert!(applied.contains("DOMAIN-SUFFIX,gosuslugi.ru,DIRECT"));
        // В стандартном режиме blanket subnet перехват не должен добавляться
        assert!(!applied.contains("SRC-IP-CIDR,192.168.0.0/16,🛡️ Whitelist"));
    }

    #[test]
    fn test_resolve_excluded_whitelist_ips_with_mac_lookup() {
        let mut cfg = crate::config::AppConfig::default();
        cfg.whitelist.devices.push(crate::config::WhitelistDevice {
            mac: "AA:BB:CC:DD:EE:01".into(),
            ip: "192.168.1.150".into(),
            name: Some("Smart TV".into()),
            enabled: true,
            server: None
        });
        cfg.whitelist.excluded_devices = vec![
            "AA:BB:CC:DD:EE:01".into(), // by MAC
            "192.168.1.151".into(),     // by IP
        ];

        let resolved = resolve_excluded_whitelist_ips(&cfg);
        assert_eq!(resolved, vec!["192.168.1.150".to_string(), "192.168.1.151".to_string()]);
    }

    #[test]
    fn test_whitelist_strict_preserves_category_ru_above_subnet_tunnel() {
        let base_yaml = r#"port: 7890
rule-providers:
  category-ru@domain: { type: http, url: "https://example.com/ru.mrs" }
  category-ru@ipcidr: { type: http, url: "https://example.com/ru_ip.mrs" }
  category-ip-geo-detect@domain: { type: http, url: "https://example.com/geo.mrs" }
proxy-groups:
  - name: PROXY
    type: select
    proxies:
      - Fastest
rules:
  - MATCH,PROXY
"#;
        let mut cfg = crate::config::AppConfig::default();
        cfg.whitelist.enabled = true;
        cfg.whitelist.strategy = crate::config::WhitelistStrategy::StrictXKeen;

        let applied = apply_whitelist_rules(base_yaml, &cfg, None);
        assert!(applied.contains(WHITELIST_BEGIN));
        assert!(applied.contains(WHITELIST_END));

        // 1. Проверяем наличие защитных отечественных правил
        assert!(applied.contains("RULE-SET,category-ip-geo-detect@domain,DIRECT"));
        assert!(applied.contains("OR,((RULE-SET,category-ru@domain),(RULE-SET,category-ru@ipcidr)),DIRECT"));

        // 2. Проверяем наличие общего перехвата подсети
        assert!(applied.contains("SRC-IP-CIDR,192.168.0.0/16,🛡️ Whitelist"));

        // 3. АНТИ-РЕГРЕССИЯ: Правила RU-сегмента обязаны физически предшествовать перехвату подсети,
        // чтобы мобильные приложения (банки, маркетплейсы, госуслуги, CDN) шли напрямую без триггера «Обнаружен VPN».
        let ru_detect_pos = applied.find("RULE-SET,category-ip-geo-detect@domain,DIRECT").expect("ru_detect must be present");
        let ru_pos = applied.find("OR,((RULE-SET,category-ru@domain),(RULE-SET,category-ru@ipcidr)),DIRECT").expect("ru must be present");
        let subnet_pos = applied.find("SRC-IP-CIDR,192.168.0.0/16,🛡️ Whitelist").expect("subnet rule must be present");

        assert!(
            ru_detect_pos < subnet_pos,
            "CRITICAL REGRESSION: category-ip-geo-detect ({}) must precede subnet tunnel ({})",
            ru_detect_pos, subnet_pos
        );
        assert!(
            ru_pos < subnet_pos,
            "CRITICAL REGRESSION: category-ru rules ({}) must precede subnet tunnel ({}) to prevent mobile VPN false-positives",
            ru_pos, subnet_pos
        );
    }

    #[test]
    fn test_ozon_and_domestic_cdns_in_whitelist_catalog() {
        let base_yaml = "port: 7890\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies:\n      - Fastest\nrules:\n  - MATCH,PROXY\n";
        let mut cfg = crate::config::AppConfig::default();
        cfg.whitelist.enabled = true;
        cfg.whitelist.strategy = crate::config::WhitelistStrategy::StrictXKeen;

        let applied = apply_whitelist_rules(base_yaml, &cfg, None);

        // Ключевые домены OZON и отечественных CDN/DDoS-фильтров обязаны присутствовать в правилах DIRECT:
        let critical_domains = [
            "ozon.ru",
            "ozonusercontent.com",
            "ozone.ru",
            "ozonru.me",
            "o3.ru",
            "o3team.ru",
            "ozon.tech",
            "ozon.travel",
            "ozon.app",
            "ozonfresh.ru",
            "ngenix.net",
            "qrator.net",
            "wbstatic.net",
            "yastatic.net",
        ];

        for &cd in &critical_domains {
            let rule = format!("DOMAIN-SUFFIX,{cd},DIRECT");
            assert!(
                applied.contains(&rule),
                "CRITICAL REGRESSION: Domain {} must have direct rule in whitelist: '{}'",
                cd, rule
            );
            assert!(
                DOMESTIC_WHITELIST_DOMAINS.contains(&cd),
                "CRITICAL REGRESSION: Domain {} must be present in DOMESTIC_WHITELIST_DOMAINS",
                cd
            );
        }
    }

    #[test]
    fn test_v1_9_30_sync_disabled_providers_in_yaml() {
        let yaml = r#"proxy-groups:
  - name: Fastest
    type: url-test
    use:
      - sub1
      - sub2
  - name: Fallback
    type: fallback
    use: [sub1, sub2]
"#;

        // 1. Отключаем sub1 -> строка комментируется
        let disabled = vec!["sub1".to_string()];
        let synced = sync_disabled_providers_in_yaml(yaml, &disabled).expect("synced");
        assert!(synced.contains("# - sub1"), "Disabled provider must be commented out: {synced}");
        assert!(synced.contains("- sub2"), "Active provider must remain active: {synced}");
        assert!(synced.contains("use: [sub2]"), "Inline use must retain only active providers: {synced}");

        // 2. Включаем sub1 обратно -> строка раскомментируется
        let none_disabled: Vec<String> = Vec::new();
        let restored = sync_disabled_providers_in_yaml(&synced, &none_disabled).expect("restored");
        assert!(restored.contains("- sub1"), "Restored provider must be uncommented: {restored}");
        assert!(!restored.contains("# - sub1"), "No commented sub1 should remain: {restored}");
        assert!(restored.contains("- sub2"));
    }

    #[test]
    fn test_v1_9_30_apply_routing_filters_disabled_providers() {
        let base_yaml = r#"proxy-providers:
  active_sub:
    type: http
    url: "https://a.b/c"
  disabled_sub:
    type: http
    url: "https://d.e/f"
proxy-groups:
  - name: PROXY
    type: select
rules:
  - MATCH,DIRECT
"#;

        let mut cfg = crate::config::AppConfig::default();
        cfg.disabled_providers = vec!["disabled_sub".to_string()];
        cfg.device_routing.insert("192.168.2.50".to_string(), crate::config::DeviceRouting {
            servers: vec!["Fastest".to_string()],
            ..Default::default()
        });

        let (applied, _) = apply_routing(base_yaml, &cfg).expect("applied");
        // Проверяем, что в созданной группе устройства нет disabled_sub в секции use:
        assert!(applied.contains("- active_sub"), "active_sub must be present in device group use");
        assert!(!applied.contains("- disabled_sub"), "disabled_sub must NOT be present in device group use: {applied}");
    }
}






