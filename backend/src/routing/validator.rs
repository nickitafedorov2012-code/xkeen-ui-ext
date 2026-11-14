use regex_lite::Regex;
use std::sync::LazyLock;

pub(crate) fn groups_name_re() -> &'static Regex {
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s{2}- name: '(.+?)'\s*$").unwrap());
    &RE
}

pub(crate) fn rules_cidr_re() -> &'static Regex {
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*- SRC-IP-CIDR,([0-9a-fA-F\.:]+)(?:/([0-9]{1,3}))?,").unwrap());
    &RE
}

pub const GROUPS_BEGIN: &str = "# --- AUTO-DEVICE-GROUPS-BEGIN ---";
pub const GROUPS_END: &str = "# --- AUTO-DEVICE-GROUPS-END ---";
pub const RULES_BEGIN: &str = "# --- AUTO-DEVICE-RULES-BEGIN ---";
pub const RULES_END: &str = "# --- AUTO-DEVICE-RULES-END ---";

pub const WHITELIST_BEGIN: &str = "# --- AUTO-WHITELIST-BEGIN ---";
pub const WHITELIST_END: &str = "# --- AUTO-WHITELIST-END ---";
pub const QUIC_REJECT_BEGIN: &str = "# --- AUTO-QUIC-REJECT-BEGIN ---";
pub const QUIC_REJECT_END: &str = "# --- AUTO-QUIC-REJECT-END ---";
pub const QUIC_REJECT_RULE: &str = "  - AND,((NETWORK,UDP),(DST-PORT,443)),REJECT";
pub const WHITELIST_DNS_BEGIN: &str = "# --- AUTO-WHITELIST-DNS-BEGIN ---";
pub const WHITELIST_DNS_END: &str = "# --- AUTO-WHITELIST-DNS-END ---";
pub const WHITELIST_GROUP_BEGIN: &str = "# --- AUTO-WHITELIST-GROUP-BEGIN ---";
pub const WHITELIST_GROUP_END: &str = "# --- AUTO-WHITELIST-GROUP-END ---";
pub const WHITELIST_GROUP_NAME: &str = "🛡️ Whitelist";

pub const CANARY_GROUP_BEGIN: &str = "# --- AUTO-CANARY-GROUP-BEGIN ---";
pub const CANARY_GROUP_END: &str = "# --- AUTO-CANARY-GROUP-END ---";
pub const CANARY_LISTENERS_BEGIN: &str = "# --- AUTO-CANARY-LISTENERS-BEGIN ---";
pub const CANARY_LISTENERS_END: &str = "# --- AUTO-CANARY-LISTENERS-END ---";
pub const CANARY_PROBE_GROUP_NAME: &str = "🛡️ Canary Probe";
pub const CANARY_PROBE_PORT: u16 = 7894;
pub const CANARY_PROBE_LISTEN: &str = "127.0.0.1";
pub const CANARY_PROBE_INBOUND_NAME: &str = "canary-probe-in";



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

/// Валидация URL health-check для провайдера (подпункт 77.2):
/// Должен быть http:// или https://, не пустой, без управляющих символов (\x00..\x1f, \x7f).
pub fn validate_provider_health_check_url(url: &str) -> Result<(), String> {
    let s = url.trim();
    if s.is_empty() {
        return Err("URL проверки доступности не может быть пустым".into());
    }
    if !s.starts_with("http://") && !s.starts_with("https://") {
        return Err("URL проверки доступности должен начинаться с http:// или https://".into());
    }
    if s.chars().any(|c| (c as u32) < 0x20 || c == '\u{7f}') {
        return Err("URL проверки доступности содержит недопустимые управляющие символы".into());
    }
    Ok(())
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

    // ID подписки и URL не могут содержать переводы строк, кавычки или запятые.
    let forbidden_chars = ['\r', '\n', '"', '\'', ','];
    if [id, url].iter().any(|s| s.chars().any(|c| forbidden_chars.contains(&c)))
        || hwid.map(|h| h.chars().any(|c| forbidden_chars.contains(&c))).unwrap_or(false)
        || user_agent.map(|u| u.chars().any(|c| forbidden_chars.contains(&c))).unwrap_or(false)
    {
        return Err("Параметры подписки содержат недопустимые символы (переводы строк, кавычки или запятые)".into());
    }

    // Защита от инъекций через обратный слэш (баг 77): слэш не запрещён, но экранируется через yaml_quote.
    let existing = parse_provider_names(yaml);
    if existing.iter().any(|p| p.eq_ignore_ascii_case(id)) {
        return Err(format!("Подписка с ID '{id}' уже существует"));
    }

    let hc_url = health_check_url.unwrap_or(PROVIDER_HEALTH_CHECK_URL);
    validate_provider_health_check_url(hc_url)?;
    let hc_interval = health_check_interval.unwrap_or(PROVIDER_HEALTH_CHECK_INTERVAL_SECS);

    let ua = user_agent.unwrap_or("ClashMeta/1.19.24; mihomo/1.19.24");
    let quoted_ua = crate::subscription_parser::yaml_quote(ua);
    let mut header_lines = format!("    header:\n      User-Agent: [{quoted_ua}]");
    if let Some(h) = hwid.map(str::trim).filter(|h| !h.is_empty()) {
        let quoted_hwid = crate::subscription_parser::yaml_quote(h);
        header_lines.push_str(&format!("\n      x-hwid: [{quoted_hwid}]"));
    }

    let is_crlf = yaml.contains("\r\n");
    let eol = if is_crlf { "\r\n" } else { "\n" };

    let quoted_url = crate::subscription_parser::yaml_quote(url);
    let quoted_hc_url = crate::subscription_parser::yaml_quote(hc_url);

    let new_block = format!(
        "  {id}:\n    type: http\n    url: {quoted_url}\n    interval: {PROVIDER_DEFAULT_INTERVAL_SECS}\n    health-check:\n      enable: true\n      lazy: true\n      url: {quoted_hc_url}\n      interval: {hc_interval}\n      expected-status: {PROVIDER_HEALTH_CHECK_EXPECTED_STATUS}\n{header_lines}\n    override:\n      udp: true\n      tfo: true"
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

    let tfo = !id.eq_ignore_ascii_case(crate::vkturn::PROVIDER_ID);
    let new_block = format!(
        "  {id}:\n    type: file\n    path: \"{file_path}\"\n    health-check:\n      enable: true\n      lazy: true\n      url: \"{hc_url}\"\n      interval: {hc_interval}\n      expected-status: {PROVIDER_HEALTH_CHECK_EXPECTED_STATUS}\n    override:\n      udp: true\n      tfo: {tfo}"
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
/// Синтаксис домена для правил DOMAIN-SUFFIX (баг 120): метки из букв, цифр, `-` и `_`, без пустых меток.
/// Запятая, скобки, кавычки и пробелы недопустимы — они ломают правило и всю секцию rules.
/// Локальные зоны (.lan/.local) разрешены: их добавляют в «Напрямую» для устройств домашней сети.
pub fn is_valid_rule_domain(d: &str) -> bool {
    !d.is_empty()
        && d.len() <= 253
        && d.contains('.')
        && d.split('.').all(|l| !l.is_empty() && l.len() <= 63 && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'))
}

/// Цель правила без символов, ломающих разбор правил Mihomo (баг 119).
pub fn is_safe_rule_target(t: &str) -> bool {
    !t.is_empty() && !t.chars().any(|c| matches!(c, ',' | '(' | ')' | '\'' | '"' | '\n' | '\r' | '#'))
}

pub fn sanitize_domains(list: &[String]) -> Vec<String> {
    let mut out: Vec<String> = list
        .iter()
        .map(|d| crate::config::normalize_domain(d))
        .filter(|d| is_valid_rule_domain(d))
        .collect();
    out.sort();
    out.dedup();
    out
}


#[cfg(test)]
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
