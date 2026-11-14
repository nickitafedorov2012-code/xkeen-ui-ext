use super::*;

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

/// Единый детектор имени группы Google AI / Flow (баг 124).
/// Матчится слово «Flow» целиком (границы слова, без регистра) либо «google» рядом с «ai»/«flow».
/// Одиночное «AI» (или «ChatGPT AI»), а также подстроки вроде «Workflow»/«Overflow» не матчатся.
pub fn is_flow_group_name(name: &str) -> bool {
    let lower = name.to_lowercase();

    // Разбиваем на алфанумерные токены (эмодзи, пробелы, дефисы и т.п. выступают разделителями)
    let tokens: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .collect();

    // 1) Слово "flow" целиком
    if tokens.iter().any(|&t| t == "flow") {
        return true;
    }

    // 2) "google" рядом с "ai" или "flow" (в соседних токенах или слитно: "googleflow", "googleai")
    if tokens.iter().any(|&t| t == "googleflow" || t == "googleai") {
        return true;
    }
    for pair in tokens.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if (a == "google" && (b == "ai" || b == "flow"))
            || ((a == "ai" || a == "flow") && b == "google")
        {
            return true;
        }
    }

    false
}

/// План починки/переключения маршрута Google Flow (баг 52).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowRepairPlan {
    pub group_name: String,
    pub target_server: String,
}

/// Валидация и планирование восстановления Google Flow (баг 52).
/// Проверяет наличие группы Flow до любой записи в config.yaml / config.json.
pub fn plan_repair_flow<S: AsRef<str>>(groups: &[S], target: &str) -> Result<FlowRepairPlan, String> {
    let target = target.trim();
    if target.is_empty() {
        return Err("Имя целевого сервера Google Flow не может быть пустым".into());
    }

    // 1. Приоритет известных точных имён групп
    for preferred in ["Google AI", "Google-AI", "GoogleFlow", "Flow"] {
        if groups.iter().any(|g| g.as_ref() == preferred && is_flow_group_name(g.as_ref())) {
            return Ok(FlowRepairPlan {
                group_name: preferred.to_string(),
                target_server: target.to_string(),
            });
        }
    }

    // 2. Любая подходящая группа по единому детектору (баг 124)
    if let Some(found) = groups.iter().find(|g| is_flow_group_name(g.as_ref())) {
        return Ok(FlowRepairPlan {
            group_name: found.as_ref().to_string(),
            target_server: target.to_string(),
        });
    }

    Err("В конфигурации ядра Mihomo не найдена группа маршрута Google Flow / Google AI".into())
}

/// План переключения сервера Google Flow (баг A-10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowSwitchPlan {
    pub group_name: String,
    pub target_server: String,
}

/// Валидация и планирование переключения сервера Google Flow (баг A-10).
/// Проверяет наличие группы Flow до любой записи в config.yaml / config.json,
/// предотвращая запись сырого имени ноды в правила Mihomo при отсутствии группы.
pub fn plan_switch_flow_server<S: AsRef<str>>(groups: &[S], target: &str) -> Result<FlowSwitchPlan, String> {
    let repair_plan = plan_repair_flow(groups, target)?;
    Ok(FlowSwitchPlan {
        group_name: repair_plan.group_name,
        target_server: repair_plan.target_server,
    })
}

/// Поиск существующей группы Flow / Google AI в YAML конфиге Mihomo.
pub fn find_flow_group_name(yaml: &str) -> Option<String> {
    let existing = parse_existing_group_names(yaml);
    // Сначала сохраняем приоритет известных точных имён при наличии нескольких подходящих групп
    for preferred in ["Google AI", "Google-AI", "GoogleFlow", "Flow"] {
        if let Some(found) = existing.iter().find(|g| g.as_str() == preferred && is_flow_group_name(g)) {
            return Some(found.clone());
        }
    }
    existing.into_iter().find(|g| is_flow_group_name(g))
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

    // Баг 23: Flow вставляется ПОСЛЕ AdBlock, пользовательских DIRECT/FORCE/DEV_DOMAINS и WHITELIST,
    // чтобы не перебивать блокировку рекламы и пользовательские правила независимо от режима БС.
    let mut insert_pos = rules_idx;
    for marker in [ADBLOCK_END, DIRECT_END, FORCE_END, DEV_DOMAINS_END, WHITELIST_END] {
        if let Some(offset) = lines[rules_idx + 1..].iter().position(|l| l.trim() == marker) {
            let candidate = rules_idx + 1 + offset;
            if candidate > insert_pos {
                insert_pos = candidate;
            }
        }
    }

    let mut out = Vec::with_capacity(lines.len() + FLOW_DOMAINS.len() + 4);
    for (i, line) in lines.iter().enumerate() {
        out.push(line.to_string());
        if i == insert_pos {
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
fn inject_adguard_into_dns_list(
    line: &str,
    key: &str,
    begin_marker: &str,
    end_marker: &str,
    agh_addr: &str,
    out: &mut Vec<String>,
) -> Result<bool, String> {
    let trimmed = line.trim();
    let prefix = format!("{key}:");
    if !trimmed.starts_with(&prefix) {
        return Ok(false);
    }
    let rest = trimmed[prefix.len()..].trim();
    // Отделяем inline-комментарий, если он есть после значения
    let rest_no_comment = if rest.starts_with('#') {
        ""
    } else {
        rest.trim()
    };
    let base_indent = line.len() - line.trim_start().len();
    let indent_str = " ".repeat(base_indent);
    let item_indent = " ".repeat(base_indent + 2);

    if rest_no_comment.is_empty() {
        out.push(line.to_string());
        out.push(format!("{}{}", item_indent, begin_marker));
        out.push(format!("{}- {}", item_indent, agh_addr));
        out.push(format!("{}{}", item_indent, end_marker));
        return Ok(true);
    }

    if rest_no_comment.starts_with('[') && rest_no_comment.ends_with(']') {
        let inner = &rest_no_comment[1..rest_no_comment.len() - 1];
        let existing: Vec<&str> = inner
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        out.push(format!("{}{}", indent_str, prefix));
        out.push(format!("{}{}", item_indent, begin_marker));
        out.push(format!("{}- {}", item_indent, agh_addr));
        out.push(format!("{}{}", item_indent, end_marker));
        for item in existing {
            out.push(format!("{}- {}", item_indent, item));
        }
        return Ok(true);
    }

    Err(format!(
        "Не удалось разобрать секцию dns.{key} в config.yaml: ожидался список, получено '{rest_no_comment}'"
    ))
}

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
        if !injected_nameserver
            && inject_adguard_into_dns_list(
                line,
                "nameserver",
                AGH_DNS_BEGIN,
                AGH_DNS_END,
                &agh_addr,
                &mut out,
            )?
        {
            injected_nameserver = true;
            continue;
        }
        if !injected_direct
            && inject_adguard_into_dns_list(
                line,
                "direct-nameserver",
                AGH_DIRECT_DNS_BEGIN,
                AGH_DIRECT_DNS_END,
                &agh_addr,
                &mut out,
            )?
        {
            injected_direct = true;
            continue;
        }
        out.push(line.to_string());
    }

    if !injected_nameserver {
        return Err(
            "Секция dns.nameserver не найдена или не распознана в config.yaml для интеграции AdGuard"
                .to_string(),
        );
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
/// Порт DNS Mihomo по умолчанию (XKeen) — используется, только если в config.yaml нет dns.listen.
pub const DEFAULT_MIHOMO_DNS_PORT: u16 = 1053;

/// Читает DNS-порт Mihomo из указанного пути `config.yaml` (баг 45).
pub fn mihomo_dns_port_from_path(config_path: &str) -> u16 {
    std::fs::read_to_string(config_path)
        .ok()
        .and_then(|y| parse_mihomo_dns_listen_port(&y))
        .unwrap_or(DEFAULT_MIHOMO_DNS_PORT)
}

/// Единая функция получения DNS-порта Mihomo из конфигурации приложения (баг 45).
pub fn mihomo_dns_port(cfg: &crate::config::AppConfig) -> u16 {
    mihomo_dns_port_from_path(&cfg.mihomo.config_path)
}

/// Порт из `dns: listen: host:port` config.yaml Mihomo (баг 45).
pub fn parse_mihomo_dns_listen_port(yaml: &str) -> Option<u16> {
    let mut in_dns = false;
    for line in yaml.lines() {
        if !line.starts_with(' ') && !line.trim().is_empty() && !line.trim_start().starts_with('#') {
            in_dns = line.trim_end() == "dns:";
            continue;
        }
        if in_dns {
            if let Some(rest) = line.trim().strip_prefix("listen:") {
                let v = rest.trim().trim_matches('"').trim_matches('\'');
                return v.rsplit(':').next().and_then(|p| p.trim().parse::<u16>().ok()).filter(|p| *p > 0);
            }
        }
    }
    None
}

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
/// Применение правил Zapret Hybrid с учетом пользовательских принудительных доменов (force_domains).
/// Любой домен, явно указанный пользователем в force_domains (PROXY), имеет абсолютный приоритет
/// и исключается из автоматических DIRECT-правил Zapret.
pub fn apply_zapret_hybrid_rules_with_force(
    yaml: &str,
    zapret_cfg: &crate::config::ZapretConfig,
    force_domains: &[String],
) -> Result<String, String> {
    apply_zapret_hybrid_rules_with_force_internal(yaml, zapret_cfg, force_domains, None)
}

/// Баг 32: Применение правил Zapret Hybrid с учетом исключённых устройств.
/// Для исключённых устройств трафик гибридных доменов не уходит в DIRECT (где Zapret в OUTPUT перехватывает его),
/// а направляется в PROXY-группу.
pub fn apply_zapret_hybrid_rules_with_force_internal(
    yaml: &str,
    zapret_cfg: &crate::config::ZapretConfig,
    force_domains: &[String],
    arp_content: Option<&str>,
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
    let flow_target = find_flow_group_name(yaml).unwrap_or_else(|| proxy_target.clone());
    let mut rules_to_add: Vec<String> = Vec::new();

    // Разрешаем IP-адреса устройств, исключённых из Zapret (баг 32)
    let excluded_ips = resolve_excluded_zapret_ips_internal(zapret_cfg, arp_content);

    // 0. Safeguard для Google AI / Antigravity (направляется в Flow-группу, а не в общий PROXY, баг 23)
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
            let rule = format!("  - DOMAIN-SUFFIX,{d},{flow_target}");
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
    let mut hybrid_targets = Vec::new();
    if zapret_cfg.hybrid_youtube {
        for d in YOUTUBE_HYBRID_DOMAINS {
            hybrid_targets.push(*d);
        }
    }

    // 2.1 Smart TV профиль -> DIRECT (оптимизация всех CDN/GGC кэшей)
    if zapret_cfg.smart_tv_mode {
        for d in SMART_TV_GGC_DOMAINS {
            hybrid_targets.push(*d);
        }
    }

    // 3. Discord -> DIRECT (минимальный пинг, прямые шлюзы)
    if zapret_cfg.hybrid_discord {
        for d in DISCORD_HYBRID_DOMAINS {
            hybrid_targets.push(*d);
        }
    }

    // 4. GitHub -> DIRECT (высокая скорость git clone и релизов без расхода VPS)
    if zapret_cfg.bypass_github {
        for d in GITHUB_DOMAINS {
            hybrid_targets.push(*d);
        }
    }

    // 5. Торренты & Трекеры -> DIRECT
    if zapret_cfg.bypass_torrents {
        for d in TORRENT_DOMAINS {
            hybrid_targets.push(*d);
        }
    }

    // 6. 18+ Контент -> DIRECT
    if zapret_cfg.bypass_adult {
        for d in ADULT_DOMAINS {
            hybrid_targets.push(*d);
        }
    }

    // 7. Пользовательские сайты и их связанные CDN (/boost) -> DIRECT
    let mut custom_strings = Vec::new();
    for entry in &zapret_cfg.custom_entries {
        if entry.enabled {
            let clean = crate::config::normalize_domain(&entry.domain);
            if !clean.is_empty() && !FLOW_DOMAINS.contains(&clean.as_str()) {
                custom_strings.push(clean);
            }
            for cdn in &entry.cdns {
                let clean_cdn = crate::config::normalize_domain(cdn);
                if !clean_cdn.is_empty() && !FLOW_DOMAINS.contains(&clean_cdn.as_str()) {
                    custom_strings.push(clean_cdn);
                }
            }
        }
    }

    for dom in hybrid_targets.iter().map(|s| s.to_string()).chain(custom_strings) {
        if !is_forced(&dom) {
            for ex_ip in &excluded_ips {
                let cidr = if ex_ip.contains('/') { ex_ip.clone() } else { format!("{ex_ip}/32") };
                let ex_rule = format!("  - AND,((SRC-IP-CIDR,{cidr}),(DOMAIN-SUFFIX,{dom})),{proxy_target}");
                if !rules_to_add.contains(&ex_rule) {
                    rules_to_add.push(ex_rule);
                }
            }
            let rule = format!("  - DOMAIN-SUFFIX,{dom},DIRECT");
            if !rules_to_add.contains(&rule) {
                rules_to_add.push(rule);
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

/// Добавляет/обновляет игровые домены в `dns.fake-ip-filter` когда `exclude_fake_ip_for_game_domains` включён (Баг 16).
/// Существующие пользовательские записи в `fake-ip-filter` не затрагиваются.
pub fn apply_gaming_fake_ip_filter(
    yaml: &str,
    cfg: &crate::config::GamingConfig,
    exclude_discord: bool,
) -> String {
    let cleaned = remove_gaming_fake_ip_filter(yaml);
    if !cfg.enabled || !cfg.exclude_fake_ip_for_game_domains {
        return cleaned;
    }

    let mut domains = get_gaming_domains_filtered(cfg, exclude_discord);
    for c in &cfg.custom_domains {
        let norm = crate::config::normalize_domain(c);
        if !norm.is_empty() && !domains.iter().any(|d| d.eq_ignore_ascii_case(&norm)) {
            domains.push(norm);
        }
    }
    if domains.is_empty() {
        return cleaned;
    }

    let mut block = Vec::with_capacity(domains.len() + 2);
    block.push(format!("    {GAMING_FAKEIP_BEGIN}"));
    for d in &domains {
        let d_clean = d.trim();
        if !d_clean.is_empty() {
            block.push(format!("    - '+.{d_clean}'"));
        }
    }
    block.push(format!("    {GAMING_FAKEIP_END}"));

    let lines: Vec<&str> = cleaned.lines().collect();
    let Some(dns_idx) = lines.iter().position(|l| l.trim_end() == "dns:") else {
        return cleaned;
    };

    // Ищем конец секции dns: (следующая строка верхнего уровня без отступа)
    let dns_end = lines
        .iter()
        .enumerate()
        .skip(dns_idx + 1)
        .find(|(_, l)| {
            let t = l.trim();
            !t.is_empty() && !t.starts_with('#') && !l.starts_with(' ') && !l.starts_with('\t')
        })
        .map(|(i, _)| i)
        .unwrap_or(lines.len());

    let fakeip_idx = lines[dns_idx + 1..dns_end]
        .iter()
        .position(|l| {
            let t = l.trim();
            t == "fake-ip-filter:" || t.starts_with("fake-ip-filter:")
        })
        .map(|rel| dns_idx + 1 + rel);

    let mut out = Vec::with_capacity(lines.len() + block.len() + 2);
    if let Some(fidx) = fakeip_idx {
        for (i, line) in lines.iter().enumerate() {
            if i == fidx {
                let t = line.trim();
                if t == "fake-ip-filter: []" || t == "fake-ip-filter:[]" {
                    out.push("  fake-ip-filter:".to_string());
                } else {
                    out.push((*line).to_string());
                }
                out.extend(block.iter().cloned());
            } else {
                out.push((*line).to_string());
            }
        }
    } else {
        for (i, line) in lines.iter().enumerate() {
            out.push((*line).to_string());
            if i == dns_idx {
                out.push("  fake-ip-filter:".to_string());
                out.extend(block.iter().cloned());
            }
        }
    }

    let mut res = out.join("\n");
    if cleaned.ends_with('\n') && !res.ends_with('\n') {
        res.push('\n');
    }
    res
}

/// Сбор всех активных доменов игрового режима из GamingConfig.
pub fn get_gaming_domains(cfg: &crate::config::GamingConfig) -> Vec<String> {
    get_gaming_domains_filtered(cfg, false)
}

/// Сбор всех активных доменов игрового режима с учетом исключения Discord (баг 15),
/// если Discord обслуживается локальным Zapret hybrid_discord (DIRECT).
pub fn get_gaming_domains_filtered(cfg: &crate::config::GamingConfig, exclude_discord: bool) -> Vec<String> {
    if !cfg.enabled {
        return Vec::new();
    }
    let mut domains = Vec::new();
    let p = &cfg.platforms;
    if p.discord && !exclude_discord {
        domains.extend(GAMING_DISCORD_DOMAINS.iter().map(|&s| s.to_string()));
    }
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

/// Имя per-device селектор-группы для игрового устройства (Баг 12).
pub fn gaming_device_group_name(ip: &str, name: &str) -> String {
    format!("🎮 {}", super::group_name_for(ip, name))
}

fn format_yaml_proxy_entry(p: &str) -> String {
    if p.contains(':') || p.contains('[') || p.contains(']') || p.contains('#') || p.contains('\'') || p.contains('"') {
        format!("      - '{}'", p.replace('\'', "''"))
    } else {
        format!("      - {p}")
    }
}

fn resolve_gaming_device_target(
    dev: &crate::config::GamingDevice,
    existing_groups: &[String],
    static_proxies: &[String],
    providers: &[String],
    extra_groups: &mut Vec<(String, Vec<String>)>,
) -> String {
    let srv = match dev.server.as_deref().map(str::trim) {
        Some(s) if !s.is_empty() && s != "default" && s != GAMING_GROUP_NAME => s,
        _ => return GAMING_GROUP_NAME.to_string(),
    };
    if matches!(srv, "DIRECT" | "REJECT" | "REJECT-DROP" | "PASS") {
        return srv.to_string();
    }
    let ip = dev.ip.trim();
    if ip.is_empty() {
        return GAMING_GROUP_NAME.to_string();
    }
    let is_static_or_group = existing_groups.iter().any(|g| g == srv)
        || static_proxies.iter().any(|p| p == srv);
    let has_providers = providers.iter().any(|p| !p.trim().is_empty());

    if !is_static_or_group && !has_providers {
        crate::log_w!(
            "Игровое устройство '{}' ({}): выбранный узел '{}' недоступен/удалён, используется группа '{}'",
            dev.name,
            ip,
            srv,
            GAMING_GROUP_NAME
        );
        return GAMING_GROUP_NAME.to_string();
    }

    let gname = gaming_device_group_name(ip, &dev.name);
    if !extra_groups.iter().any(|(n, _)| n == &gname) {
        let mut lines = Vec::new();
        lines.push(format!("  - name: '{}'", gname.replace('\'', "''")));
        lines.push("    type: select".to_string());
        lines.push("    proxies:".to_string());
        if is_static_or_group {
            lines.push(format_yaml_proxy_entry(srv));
        }
        lines.push(format_yaml_proxy_entry(GAMING_GROUP_NAME));
        lines.push("      - DIRECT".to_string());
        if has_providers {
            lines.push("    use:".to_string());
            for p in providers {
                let p = p.trim();
                if !p.is_empty() {
                    lines.push(format!("      - {p}"));
                }
            }
        }
        extra_groups.push((gname.clone(), lines));
    }
    gname
}

/// Применение выделенного маршрута игрового режима (селектор-группа 🎮 Gaming и правила rules:).
pub fn apply_gaming_rules(
    yaml: &str,
    cfg: &crate::config::GamingConfig,
    providers: &[String],
) -> Result<String, String> {
    apply_gaming_rules_with_zapret(yaml, cfg, providers, false)
}

/// Применение выделенного маршрута игрового режима с учетом Zapret hybrid_discord (баг 15).
pub fn apply_gaming_rules_with_zapret(
    yaml: &str,
    cfg: &crate::config::GamingConfig,
    providers: &[String],
    exclude_discord: bool,
) -> Result<String, String> {
    crate::routing::validate_marker_pair(yaml, GAMING_BEGIN, GAMING_END)?;
    crate::routing::validate_marker_pair(yaml, GAMING_GROUP_BEGIN, GAMING_GROUP_END)?;
    let content = remove_gaming_rules(yaml);
    if !cfg.enabled {
        return Ok(apply_gaming_fake_ip_filter(&content, cfg, exclude_discord));
    }

    let target_srv = if cfg.target_server.trim().is_empty() {
        "Fastest"
    } else {
        cfg.target_server.trim()
    };

    let existing_groups = parse_existing_group_names(&content);
    let static_proxies = parse_static_proxy_names(&content);
    let has_providers_section = content.contains("proxy-providers:");

    // Заранее строим per-device select-группы и правила для активных устройств (Баг 12)
    let mut extra_groups: Vec<(String, Vec<String>)> = Vec::new();
    let mut gaming_rules = Vec::new();
    match cfg.mode {
        crate::config::GamingMode::Compatibility => {
            let mut routed_any = false;
            for dev in &cfg.devices {
                if !dev.enabled {
                    continue;
                }
                let target = resolve_gaming_device_target(
                    dev,
                    &existing_groups,
                    &static_proxies,
                    providers,
                    &mut extra_groups,
                );
                let ip = dev.ip.trim();
                if ip.is_empty() {
                    crate::log_w!("Игровое устройство '{}' (MAC {}) пропущено в Compatibility-правилах: пустой IP", dev.name, dev.mac);
                } else {
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
            if !routed_any {
                for c in &cfg.custom_domains {
                    let t = c.trim().to_lowercase();
                    if !t.is_empty() {
                        gaming_rules.push(format!("  - DOMAIN-SUFFIX,{t},{GAMING_GROUP_NAME}"));
                    }
                }
                let domains = get_gaming_domains_filtered(cfg, exclude_discord);
                for d in &domains {
                    gaming_rules.push(format!("  - DOMAIN-SUFFIX,{d},{GAMING_GROUP_NAME}"));
                }
            }
        }
        crate::config::GamingMode::BypassRu => {
            let mut routed_any = false;
            let has_ru_ruleset = content.contains("category-ru@domain");
            let has_ip_ruleset = content.contains("category-ru@ipcidr");
            for dev in &cfg.devices {
                if !dev.enabled {
                    continue;
                }
                let target = resolve_gaming_device_target(
                    dev,
                    &existing_groups,
                    &static_proxies,
                    providers,
                    &mut extra_groups,
                );
                let ip = dev.ip.trim();
                if ip.is_empty() {
                    crate::log_w!("Игровое устройство '{}' (MAC {}) пропущено в BypassRu-правилах: пустой IP", dev.name, dev.mac);
                } else {
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
                let domains = get_gaming_domains_filtered(cfg, exclude_discord);
                for d in &domains {
                    gaming_rules.push(format!("  - DOMAIN-SUFFIX,{d},{GAMING_GROUP_NAME}"));
                }
            }
        }
        crate::config::GamingMode::KnownServices | crate::config::GamingMode::SmartSplit => {
            let domains = get_gaming_domains_filtered(cfg, exclude_discord);
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

    // 1. Вставка селектор-группы в proxy-groups:
    let lines: Vec<&str> = content.lines().collect();
    let pg_idx = lines
        .iter()
        .position(|l| l.trim_end() == "proxy-groups:")
        .ok_or("В config.yaml нет секции proxy-groups:")?;

    let mut with_group = Vec::with_capacity(lines.len() + 16 + extra_groups.len() * 10);
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
                with_group.push(format_yaml_proxy_entry(p));
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
            for (_, grp_lines) in &extra_groups {
                with_group.extend(grp_lines.iter().cloned());
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

    Ok(apply_gaming_fake_ip_filter(&out.join("\n"), cfg, exclude_discord))
}

const GAMING_FAKEIP_BEGIN: &str = "# --- XKEEN-GAMING-FAKEIP-BEGIN ---";
const GAMING_FAKEIP_END: &str = "# --- XKEEN-GAMING-FAKEIP-END ---";

/// Удаление блока игровых доменов из `dns.fake-ip-filter`.
pub fn remove_gaming_fake_ip_filter(yaml: &str) -> String {
    let mut out = Vec::new();
    let mut skipping = false;
    for line in yaml.lines() {
        let t = line.trim();
        if t == GAMING_FAKEIP_BEGIN {
            skipping = true;
            continue;
        }
        if t == GAMING_FAKEIP_END {
            skipping = false;
            continue;
        }
        if !skipping {
            out.push(line);
        }
    }
    let mut res = out.join("\n");
    if yaml.ends_with('\n') && !res.ends_with('\n') {
        res.push('\n');
    }
    res
}

/// Применение правил Zapret Hybrid (YouTube DIRECT, Discord DIRECT, Изоляция PROXY).
#[cfg(test)]
pub fn apply_zapret_hybrid_rules(yaml: &str, zapret_cfg: &crate::config::ZapretConfig) -> Result<String, String> {
    apply_zapret_hybrid_rules_with_force(yaml, zapret_cfg, &[])
}
