use crate::config::AppConfig;

/// Проверяет, содержит ли строка явный маркер белых списков (`[БС]`, `[BS]`, `[WL]`,
/// `белые списки`, или отдельное слово `whitelist`/`wl`/`бс`/`bs` с границами слов).
pub fn has_bs_tag(s: &str) -> bool {
    let lower = s.to_lowercase();
    if lower.contains("[бс]")
        || lower.contains("(бс)")
        || lower.contains("[бc]")
        || lower.contains("(бc)")
        || lower.contains("[бs]")
        || lower.contains("(бs)")
        || lower.contains("[bs]")
        || lower.contains("(bs)")
        || lower.contains("[wl]")
        || lower.contains("(wl)")
        || lower.contains("белые списки")
        || lower.contains("белый список")
    {
        return true;
    }
    lower
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| matches!(w, "whitelist" | "wl" | "бс" | "bs"))
}

/// Проверяет, входит ли подписка/провайдер (с учётом алиасов) в `cfg.whitelist_providers`.
pub fn provider_matches_whitelist(provider: &str, cfg: &AppConfig) -> bool {
    let p_trim = provider.trim();
    if p_trim.is_empty() {
        return false;
    }
    let p_lower = p_trim.to_lowercase();
    let alias_lower = cfg
        .provider_aliases
        .iter()
        .find(|(k, _)| k.trim().to_lowercase() == p_lower)
        .map(|(_, v)| v.trim().to_lowercase());
    let id_from_alias_lower = cfg
        .provider_aliases
        .iter()
        .find(|(_, v)| v.trim().to_lowercase() == p_lower)
        .map(|(k, _)| k.trim().to_lowercase());

    cfg.whitelist_providers.iter().any(|wp| {
        let wp_lower = wp.trim().to_lowercase();
        if wp_lower.is_empty() {
            return false;
        }
        wp_lower == p_lower
            || alias_lower.as_deref() == Some(wp_lower.as_str())
            || id_from_alias_lower.as_deref() == Some(wp_lower.as_str())
    })
}

/// Единая функция определения принадлежности узла к БС (Баг 42, Баг 99, Баг 132).
/// Порядок: `normal_servers` -> `whitelist_servers` -> `whitelist_providers` -> `has_bs_tag`.
pub fn is_whitelist_server_full(
    node_name: &str,
    node_id: &str,
    provider: Option<&str>,
    cfg: &AppConfig,
) -> bool {
    if (!node_id.is_empty() && cfg.normal_servers.iter().any(|s| s == node_id))
        || (!node_name.is_empty() && cfg.normal_servers.iter().any(|s| s == node_name))
    {
        return false;
    }
    if !node_id.is_empty() && cfg.whitelist_servers.iter().any(|s| s == node_id) {
        return true;
    }
    if let Some(p) = provider {
        if provider_matches_whitelist(p, cfg) {
            return true;
        }
    }
    has_bs_tag(node_name) || (!node_id.is_empty() && has_bs_tag(node_id))
}

/// Проверка записи цепочки при миграции конфига (учитывает `provider:node`, `normal_servers`,
/// `whitelist_servers`, `whitelist_providers` и теги БС).
pub fn is_whitelist_chain_entry(entry: &str, cfg: &AppConfig) -> bool {
    let inferred_provider = entry.split_once(':').map(|(prov, _)| prov);
    is_whitelist_server_full(entry, entry, inferred_provider, cfg)
}
