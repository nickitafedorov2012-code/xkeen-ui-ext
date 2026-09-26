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
            if trimmed.starts_with("url:") {
                if let Some(ref p) = cur_provider {
                    let val = trimmed.trim_start_matches("url:").trim().trim_matches('"').trim_matches('\'').to_string();
                    if !val.is_empty() {
                        out.insert(p.clone(), val);
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

    let new_block = format!(
        "  {id}:\n    type: http\n    url: \"{url}\"\n    interval: {PROVIDER_DEFAULT_INTERVAL_SECS}\n    health-check:\n      enable: true\n      lazy: true\n      url: \"{hc_url}\"\n      interval: {hc_interval}\n      expected-status: {PROVIDER_HEALTH_CHECK_EXPECTED_STATUS}\n{header_lines}\n    override:\n      udp: true\n      tfo: true"
    );

    let mut out = Vec::new();
    let mut in_providers = false;
    let mut inserted = false;

    for line in yaml.lines() {
        let trimmed = line.trim();
        let is_top = !line.starts_with(' ') && !trimmed.is_empty();

        if line.trim_end() == "proxy-providers:" {
            in_providers = true;
            out.push(line.to_string());
            out.push(new_block.clone());
            inserted = true;
            continue;
        }
        if in_providers && is_top {
            in_providers = false;
        }
        out.push(line.to_string());
    }

    if !inserted {
        out.push("\nproxy-providers:".to_string());
        out.push(new_block);
    }

    Ok(out.join("\n"))
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

    Ok(out.join("\n"))
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
    "PROXY".to_string()
}

/// Применение правил Zapret Hybrid (YouTube DIRECT, Discord DIRECT, Изоляция PROXY).
pub fn apply_zapret_hybrid_rules(yaml: &str, zapret_cfg: &crate::config::ZapretConfig) -> Result<String, String> {
    crate::routing::validate_marker_pair(yaml, ZAPRET_HYBRID_BEGIN, ZAPRET_HYBRID_END)?;
    let content = remove_zapret_hybrid_rules(yaml);
    if !zapret_cfg.enabled {
        return Ok(content);
    }

    let proxy_target = find_proxy_target_group(yaml);
    let mut rules_to_add: Vec<String> = Vec::new();

    // 1. Изоляция IP-блокировок (ChatGPT, Claude, X/Twitter, Instagram -> PROXY)
    if zapret_cfg.isolated_proxy {
        for d in ISOLATED_PROXIED_DOMAINS {
            rules_to_add.push(format!("  - DOMAIN-SUFFIX,{d},{proxy_target}"));
        }
    }

    // 2. YouTube -> DIRECT (максимальная скорость с локальных кэшей GGC)
    if zapret_cfg.hybrid_youtube {
        for d in YOUTUBE_HYBRID_DOMAINS {
            rules_to_add.push(format!("  - DOMAIN-SUFFIX,{d},DIRECT"));
        }
    }

    // 3. Discord -> DIRECT (минимальный пинг, прямые шлюзы)
    if zapret_cfg.hybrid_discord {
        for d in DISCORD_HYBRID_DOMAINS {
            rules_to_add.push(format!("  - DOMAIN-SUFFIX,{d},DIRECT"));
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

    let mut out = Vec::with_capacity(lines.len() + rules_to_add.len() + 4);
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
            group_proxies.push(target_srv.to_string());
            for p in &["Fastest", "PROXY", "Fallback", "DIRECT"] {
                if !group_proxies.iter().any(|x| x == *p) {
                    group_proxies.push(p.to_string());
                }
            }
            for p in &group_proxies {
                with_group.push(format!("      - {p}"));
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
            // Режим совместимости: направляет весь интернет-трафик устройства через игровой туннель
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
                let ip = dev.ip.trim();
                if !ip.is_empty() {
                    let cidr = if ip.contains('/') { ip.to_string() } else { format!("{ip}/32") };
                    gaming_rules.push(format!("  - SRC-IP-CIDR,{cidr},{GAMING_GROUP_NAME}"));
                    routed_any = true;
                }
                for v6 in &dev.ipv6 {
                    let v6 = v6.trim();
                    let v6_lower = v6.to_lowercase();
                    if !v6.is_empty() && !v6_lower.starts_with("fe80:") && !v6_lower.starts_with("::1") {
                        let cidr = if v6.contains('/') { v6.to_string() } else { format!("{v6}/128") };
                        gaming_rules.push(format!("  - SRC-IP-CIDR,{cidr},{GAMING_GROUP_NAME}"));
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
        crate::config::GamingMode::KnownServices | crate::config::GamingMode::SmartSplit => {
            // Режим «Только известные игровые сервисы» на базе category-games и правил платформ
            let domains = get_gaming_domains(cfg);
            for d in &domains {
                gaming_rules.push(format!("  - DOMAIN-SUFFIX,{d},{GAMING_GROUP_NAME}"));
            }
            if cfg.platforms.category_games {
                gaming_rules.push(format!("  - GEOSITE,category-games,{GAMING_GROUP_NAME}"));
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
    let force = sanitize_domains(force);
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
                    for r in rules {
                        let dom = r.domain.trim().to_lowercase();
                        let target = r.target.trim();
                        if !dom.is_empty() && !target.is_empty() {
                            out.push(format!(
                                "  - AND,((SRC-IP-CIDR,{}/32),(DOMAIN-SUFFIX,{})),{},no-resolve",
                                ip, dom, target
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

            // 3. Принудительные домены (PROXY, исключая специализированные Google AI домены)
            if !force.is_empty() {
                out.push(FORCE_BEGIN.to_string());
                for d in &force {
                    if !FLOW_DOMAINS.iter().any(|g| g.eq_ignore_ascii_case(d)) {
                        out.push(format!("  - DOMAIN-SUFFIX,{d},PROXY"));
                    }
                }
                out.push(FORCE_END.to_string());
            }
        }
    }
    Ok(out.join("\n"))
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
        let is_group_start = trimmed.starts_with("- name:") && indent_len <= 2;
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

/// Применение всей сохраненной в AppConfig маршрутизации к сырому YAML Mihomo.
pub fn apply_routing(yaml: &str, cfg: &crate::config::AppConfig) -> Result<(String, usize), String> {
    let mut current = yaml.to_string();

    // 0. Блокировка рекламы на роутере (AdBlock)
    if let Ok(with_adblock) = apply_adblock_rules(&current, cfg.adblock_enabled) {
        current = with_adblock;
    }

    // 1. Умные гибридные правила Zapret (YouTube / Discord -> DIRECT, AI -> PROXY)
    current = apply_zapret_hybrid_rules(&current, &cfg.zapret)?;

    // 2. Доменные правила (DIRECT / FORCE / PER-DEVICE DOMAINS)
    let direct = &cfg.direct_domains;
    let force = &cfg.force_domains;
    let device_domains = &cfg.device_domain_rules;
    current = apply_domain_rules(&current, direct, force, device_domains)?;

    // 3. Выделенный маршрут Google Flow & AI (высший приоритет — на самом верху секции rules)
    let flow_target = cfg.flow_server.as_deref().filter(|s| !s.trim().is_empty()).unwrap_or("PROXY");
    let flow_group = find_flow_group_name(&current);
    if let Ok(with_flow) = apply_flow_rules(&current, flow_target.trim(), flow_group.as_deref()) {
        current = with_flow;
    }

    let providers = if !cfg.mihomo.device_providers.is_empty() {
        cfg.mihomo.device_providers.clone()
    } else {
        parse_provider_names(&current)
    };

    // 4. Per-device назначения
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

    let count = assignments.len();
    if let Ok(with_devices) = apply_assignments(&current, &assignments, &providers) {
        current = with_devices;
    }

    // 5. Игнор-лист
    if let Ok(with_ig) = apply_ignore_to_groups(&current, &cfg.ignore_servers) {
        current = with_ig;
    }
    let mut filters = cfg.provider_filters.clone();
    current = apply_ignore_to_providers(&current, &cfg.ignore_servers, &mut filters);

    // 6. Игровой режим (приоритетный маршрут устройства и селектор-группа 🎮 Gaming)
    current = apply_gaming_rules(&current, &cfg.gaming, &providers)?;

    Ok((current, count))
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
    fn apply_ignore_to_groups_4_space_indentation() {
        let yaml = "proxy-groups:\n    - name: Fastest\n      type: url-test\n      proxies:\n        - Server1\n        - Server2\n";
        let ignore = vec!["Server1".to_string()];
        let out = apply_ignore_to_groups(yaml, &ignore).unwrap();
        assert!(out.contains("exclude-filter: 'Server1'"));
    }

    #[test]
    fn apply_routing_empty_config_yaml() {
        let cfg = crate::config::AppConfig::default();
        let (out, count) = apply_routing("", &cfg);
        assert_eq!(out, "");
        assert_eq!(count, 0);
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
        };

        // 1. Включение всех гибридных правил
        let with_zapret = apply_zapret_hybrid_rules(yaml, &zapret_cfg).unwrap();
        assert!(with_zapret.contains(ZAPRET_HYBRID_BEGIN));
        assert!(with_zapret.contains("DOMAIN-SUFFIX,googlevideo.com,DIRECT"));
        assert!(with_zapret.contains("DOMAIN-SUFFIX,discord.com,DIRECT"));
        assert!(with_zapret.contains("DOMAIN-SUFFIX,openai.com,PROXY"));
        assert!(with_zapret.contains("DOMAIN-SUFFIX,example.com,DIRECT"));
        // DOMAIN-KEYWORD правила НЕ должны присутствовать (слишком широкие)
        assert!(!with_zapret.contains("DOMAIN-KEYWORD,youtube"));
        assert!(!with_zapret.contains("DOMAIN-KEYWORD,discord"));

        // 2. Идемпотентность
        let dup = apply_zapret_hybrid_rules(&with_zapret, &zapret_cfg).unwrap();
        assert_eq!(dup.matches(ZAPRET_HYBRID_BEGIN).count(), 1);

        // 3. Выключение YouTube (возврат в PROXY) — Discord и isolated_proxy остаются
        zapret_cfg.hybrid_youtube = false;
        let no_yt = apply_zapret_hybrid_rules(&dup, &zapret_cfg).unwrap();
        assert!(!no_yt.contains("googlevideo.com,DIRECT"));
        assert!(no_yt.contains("discord.com,DIRECT"));
        assert!(no_yt.contains("openai.com,PROXY"));

        // 4. Полное отключение службы Zapret
        zapret_cfg.enabled = false;
        let disabled = apply_zapret_hybrid_rules(&no_yt, &zapret_cfg).unwrap();
        assert!(!disabled.contains(ZAPRET_HYBRID_BEGIN));
        assert!(!disabled.contains("discord.com,DIRECT"));
        assert!(!disabled.contains("openai.com"));
        assert!(disabled.contains("DOMAIN-SUFFIX,example.com,DIRECT"));
    }

    #[test]
    fn test_remove_adblock_missing_end_marker_does_not_truncate() {
        let yaml_with_missing_end = format!(
            "rules:\n{}\n  - GEOSITE,category-ads-all,REJECT\n  - GEOIP,RU,DIRECT\n  - MATCH,PROXY\n",
            ADBLOCK_BEGIN
        );
        let result = remove_adblock_rules(&yaml_with_missing_end);
        assert_eq!(result, yaml_with_missing_end);
        assert!(result.contains("GEOIP,RU,DIRECT"));
        assert!(result.contains("MATCH,PROXY"));
    }

    #[test]
    fn test_remove_flow_missing_end_marker_does_not_truncate() {
        let yaml_with_missing_end = format!(
            "rules:\n{}\n  - DOMAIN-SUFFIX,google.com,FLOW\n  - GEOIP,RU,DIRECT\n",
            FLOW_BEGIN
        );
        let result = remove_flow_rules(&yaml_with_missing_end);
        assert_eq!(result, yaml_with_missing_end);
        assert!(result.contains("GEOIP,RU,DIRECT"));
    }

    #[test]
    fn test_remove_zapret_hybrid_missing_end_marker_does_not_truncate() {
        let yaml_with_missing_end = format!(
            "rules:\n{}\n  - DOMAIN-SUFFIX,youtube.com,DIRECT\n  - MATCH,PROXY\n",
            ZAPRET_HYBRID_BEGIN
        );
        let result = remove_zapret_hybrid_rules(&yaml_with_missing_end);
        assert_eq!(result, yaml_with_missing_end);
        assert!(result.contains("MATCH,PROXY"));
    }

    #[test]
    fn test_remove_domain_blocks_missing_end_marker_does_not_truncate() {
        let yaml_with_missing_end = format!(
            "rules:\n{}\n  - DOMAIN,blocked.com,REJECT\n  - MATCH,PROXY\n",
            DOMAIN_BLOCKS_BEGIN
        );
        let result = remove_domain_blocks(&yaml_with_missing_end);
        assert_eq!(result, yaml_with_missing_end);
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
        });

        let applied = apply_gaming_rules(yaml, &cfg, &[]).expect("applied");
        assert!(applied.contains("SRC-IP-CIDR,192.168.2.50/32,🎮 Gaming"));
        assert!(applied.contains("SRC-IP-CIDR,2a02:1234:5678::1/128,🎮 Gaming"));
        assert!(!applied.contains("fe80::1ff:fe00:1"), "fe80: must be excluded");
        assert!(!applied.contains("::1/128"), "loopback must be excluded");
    }

    #[test]
    fn test_apply_routing_gaming_priority_over_device_assignment() {
        let yaml = "port: 7890\nproxy-groups:\nrules:\n  - MATCH,PROXY\n";
        let mut app_cfg = crate::config::AppConfig::default();
        app_cfg.device_routing.insert(
            "192.168.2.115".into(),
            crate::config::DeviceRoute {
                servers: vec!["OldProxy".into()],
                direct: false,
                last_active: 0,
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
        });
        cfg.devices.push(crate::config::GamingDevice {
            mac: "22:22:22:22:22:22".into(),
            ip: "192.168.2.20".into(),
            ipv6: vec![],
            name: "Inactive-PC".into(),
            enabled: false,
        });

        let applied = apply_gaming_rules(yaml, &cfg, &[]).expect("applied");
        assert!(applied.contains("SRC-IP-CIDR,192.168.2.10/32,🎮 Gaming"));
        assert!(!applied.contains("SRC-IP-CIDR,192.168.2.20/32,🎮 Gaming"), "Disabled device must not be routed");
    }
}


