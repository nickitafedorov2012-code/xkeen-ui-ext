use super::*;

/// Извлечение порта смешанного прокси (mixed-port) из config.yaml Mihomo (баг 115)
pub fn parse_mixed_port(yaml: &str) -> Option<u16> {
    for line in yaml.lines() {
        let t = line.trim();
        if let Some(val_str) = t.strip_prefix("mixed-port:") {
            let s = val_str.trim().trim_matches('\'').trim_matches('"');
            if let Ok(p) = s.parse::<u16>() {
                if p > 0 {
                    return Some(p);
                }
            }
        }
    }
    None
}

/// Получение mixed-port из файла config.yaml Mihomo (дефолт 7890 при отсутствии)
pub fn mihomo_mixed_port_from_path(yaml_path: &str) -> u16 {
    if let Ok(yaml) = std::fs::read_to_string(yaml_path) {
        if let Some(port) = parse_mixed_port(&yaml) {
            return port;
        }
    }
    7890
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortSource {
    FromConfig,
    Default,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RedirPorts {
    pub redir: u16,
    pub tproxy: u16,
    pub source: PortSource,
}

static REDIR_PORT_FALLBACK_WARNED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Единый источник портов redir/tproxy из config.yaml с однократным предупреждением при фоллбэке.
pub fn redir_ports(yaml: &str) -> RedirPorts {
    let r = parse_redir_port(yaml);
    let t = parse_tproxy_port(yaml);
    let source = if r.is_some() && t.is_some() {
        PortSource::FromConfig
    } else {
        PortSource::Default
    };
    let redir = r.unwrap_or(5000);
    let tproxy = t.unwrap_or(5001);
    if source == PortSource::Default
        && !REDIR_PORT_FALLBACK_WARNED.swap(true, std::sync::atomic::Ordering::Relaxed)
    {
        crate::log_w!(
            "[ROUTING] В config.yaml не найдены порты redir-port/tproxy-port — используются значения по умолчанию ({redir}/{tproxy})"
        );
    }
    RedirPorts { redir, tproxy, source }
}
/// Применение всей сохраненной в AppConfig маршрутизации к сырому YAML Mihomo с сохранением фильтров провайдеров.
pub fn apply_routing_with_filters(
    yaml: &str,
    cfg: &crate::config::AppConfig,
    filters: &mut std::collections::BTreeMap<String, String>,
) -> Result<(String, usize), String> {
    let mut current = sync_disabled_providers_in_yaml(yaml, &cfg.disabled_providers).unwrap_or_else(|_| yaml.to_string());
    let strict_whitelist_active = cfg.whitelist.enabled
        && cfg.whitelist.strategy == crate::config::WhitelistStrategy::StrictXKeen;
    if strict_whitelist_active {
        // StrictXKeen rebuilds these blocks after all generated routing rules. Strip
        // the previous copy first so a panel/core restart produces the same priority.
        current = remove_domain_blocks(&current);
    }

    let providers = if !cfg.mihomo.device_providers.is_empty() {
        cfg.mihomo.device_providers.clone()
    } else {
        parse_provider_names(&current)
    };
    let providers: Vec<String> = providers
        .into_iter()
        .filter(|p| !cfg.disabled_providers.iter().any(|dp| dp.eq_ignore_ascii_case(p)))
        .collect();

    // 1. Per-device назначения (базовый маршрут уровня устройства: SRC-IP-CIDR).
    // В обычном режиме они создаются первыми; при StrictXKeen блок пересоздаётся
    // после auto-правил, но до Whitelist catch-all.
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
    for d in &cfg.gaming.devices {
        let is_active = d.enabled;
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
    let pinned = pinned_whitelist_ips(cfg);
    if !strict_whitelist_active {
        if let Ok(with_devices) = apply_assignments(&current, &assignments, &providers, &pinned) {
            current = with_devices;
        }
    }

    // 2. Игнор-лист
    if let Ok(with_ig) = apply_ignore_to_groups(&current, &cfg.ignore_servers) {
        current = with_ig;
    }
    current = apply_ignore_to_providers(&current, &cfg.ignore_servers, filters);

    // 3. Игровой режим (приоритетный маршрут устройства и селектор-группа 🎮 Gaming)
    // Баг 15: если Zapret hybrid_discord активен, Discord направляется в DIRECT и исключается из игровых правил
    let exclude_discord = cfg.zapret.enabled && cfg.zapret.hybrid_discord;
    current = apply_gaming_rules_with_zapret(&current, &cfg.gaming, &providers, exclude_discord)?;

    let active_force = &cfg.force_domains;

    // 4. Умные гибридные правила Zapret (YouTube / Discord / GitHub / Torrents / Adult -> DIRECT, AI -> PROXY)
    // Пользовательские force_domains имеют абсолютный приоритет над DIRECT-правилами Zapret
    current = apply_zapret_hybrid_rules_with_force(&current, &cfg.zapret, active_force)?;

    // В StrictXKeen пользовательские назначения устройств ставим после auto-блоков
    // игровых режимов/Zapret и до catch-all, чтобы они не теряли свой заданный маршрут.
    if strict_whitelist_active {
        if let Ok(with_devices) = apply_assignments(&current, &assignments, &providers, &pinned) {
            current = with_devices;
        }
    }

    // 5. Доменные правила (DIRECT / FORCE / PER-DEVICE DOMAINS).
    // В обычном режиме сохраняется прежний порядок. В StrictXKeen эти блоки
    // накладываются после системных правил ниже, чтобы два списка настроек были первыми.
    let direct = &cfg.direct_domains;
    let device_domains = &cfg.device_domain_rules;
    if !strict_whitelist_active {
        current = apply_domain_rules(&current, direct, active_force, device_domains)?;
    }

    // 5.1. Режим белых списков: в StrictXKeen LAN catch-all следует после
    // явных назначений устройств и до остальных автоматически сгенерированных правил.
    let active_wl_tunnel = match cfg.whitelist.target_server.as_deref() {
        Some(s) if !s.trim().is_empty() && s.trim() != "follow_failover" => Some(s.trim()),
        _ => None,
    };
    current = apply_whitelist_rules(&current, cfg, active_wl_tunnel);
    current = apply_whitelist_dns(&current, cfg, "127.0.0.1:53");

    // 5.2. Canary Probe: селектор-группа '🛡️ Canary Probe' и локальный слушатель 127.0.0.1:7894.
    // Нужна и failover (Canary-тест), и изолированному speedtest (баг 84), поэтому есть всегда.
    current = apply_canary_rules_with_providers(&current, &providers);

    // 5.3. Блокировка рекламы на роутере (AdBlock) — применяется ко всем устройствам
    if let Ok(with_adblock) = apply_adblock_rules(&current, cfg.adblock_enabled) {
        current = with_adblock;
    }

    // 5.4. Интеграция AdGuard Home DNS под прокси (Mihomo nameserver -> 127.0.0.1:5353)
    let agh_enabled = cfg.adguard.enabled || cfg.adblock_enabled;
    let agh_port = if cfg.adguard.dns_port > 0 { cfg.adguard.dns_port } else { 5354 };
    if let Ok(with_agh_dns) = apply_adguard_dns_upstream(&current, agh_port, agh_enabled) {
        current = with_agh_dns;
    }

    // 6. Выделенный маршрут Google Flow & AI (высший приоритет — на самом верху секции rules)
    let flow_target = cfg.flow_server.as_deref().filter(|s| !s.trim().is_empty()).unwrap_or("PROXY");
    let flow_group = find_flow_group_name(&current);
    if let Ok(with_flow) = apply_flow_rules(&current, flow_target.trim(), flow_group.as_deref()) {
        current = with_flow;
    }

    // 7. Правила прямого обхода VK-туннеля (TURN ВК / Телемост и VPS peer) в самый верх rules:
    if let Ok(with_vkturn) = crate::vkturn::apply_rules(&current, &cfg.vkturn) {
        current = with_vkturn;
    }

    // В режиме StrictXKeen пользовательские списки direct_domains/force_domains/device_domains
    // накладываются выше блока БС, но не должны вести в VPS-группу PROXY (INV-1, баг 3):
    // любые не-DIRECT/REJECT цели заменяются на группу 🛡️ Whitelist.
    if strict_whitelist_active {
        current = apply_domain_rules_with_priority_for_target(
            &current,
            direct,
            active_force,
            device_domains,
            true,
            Some(WHITELIST_GROUP_NAME),
        )?;
    }

    // DNS-HE-01: параллельный TCP-дозвон Happy Eyeballs (RFC 8305) для исключения задержек YouTube
    current = ensure_tcp_concurrent(&current);

    // QUIC-ORD-01: управляемое отклонение QUIC строго ДО белых списков и ДО всех SRC-IP-CIDR
    current = apply_quic_reject_rules(&current);

    // INV-4 (баги 12, 39, 51): страховка — цель правила обязана быть группой, статическим прокси или
    // встроенной целью. Иначе Mihomo отвергает весь конфиг. Неизвестная цель заменяется группой блока.
    let (sanitized, replaced) = sanitize_rule_targets(&current);
    for r in &replaced {
        crate::log_w!("[ROUTING] Цель правила '{r}' не является группой или прокси — заменена (INV-4)");
    }
    current = sanitized;

    Ok((current, count))
}

/// Цель правила Mihomo из строки `- TYPE,PAYLOAD,TARGET[,опции]` (MATCH — второй элемент,
/// логические AND/OR/NOT — после закрывающей скобки).
pub fn rule_target_of(line: &str) -> Option<String> {
    let body = line.strip_prefix("- ")?.trim();
    if body.starts_with('#') || body.is_empty() {
        return None;
    }
    let upper = body.to_ascii_uppercase();
    if upper.starts_with("SUB-RULE,") {
        return None;
    }
    if upper.starts_with("AND,") || upper.starts_with("OR,") || upper.starts_with("NOT,") {
        let close = body.rfind("),")?;
        return body[close + 2..].split(',').next().map(|t| t.trim().to_string());
    }
    let parts: Vec<&str> = body.split(',').collect();
    if upper.starts_with("MATCH,") {
        return parts.get(1).map(|t| t.trim().to_string());
    }
    parts.get(2).map(|t| t.trim().to_string())
}

/// Замена недопустимых целей правил (баги 12, 39, 51). В блоке БС — на 🛡️ Whitelist (не допускаем
/// третий канал), в блоке гейминга — на 🎮 Gaming, иначе — на основную прокси-группу.
pub fn sanitize_rule_targets(yaml: &str) -> (String, Vec<String>) {
    let groups = parse_existing_group_names(yaml);
    let statics = parse_static_proxy_names(yaml);
    let builtin = ["DIRECT", "REJECT", "REJECT-DROP", "PASS", "COMPATIBLE", "GLOBAL"];
    let is_valid = |t: &str| {
        let tq = t.trim_matches('\'').trim_matches('"');
        builtin.contains(&tq) || groups.iter().any(|g| g == tq) || statics.iter().any(|g| g == tq)
    };
    let main_fallback = find_proxy_target_group(yaml);
    let has_group = |g: &str| groups.iter().any(|x| x == g);
    let mut replaced = Vec::new();
    let mut out = Vec::new();
    let mut in_rules = false;
    let mut block: Option<&str> = None;
    let mut in_auto = false;
    for line in yaml.lines() {
        if !line.starts_with(' ') && !line.trim().is_empty() && !line.trim_start().starts_with('#') {
            in_rules = line.trim_end() == "rules:";
        }
        let t = line.trim();
        if t.starts_with("# --- AUTO-") && t.ends_with("-BEGIN ---") {
            in_auto = true;
        } else if t.starts_with("# --- AUTO-") && t.ends_with("-END ---") {
            in_auto = false;
        }
        if t == WHITELIST_BEGIN {
            block = Some("wl");
        } else if t == GAMING_BEGIN {
            block = Some("gaming");
        } else if t == WHITELIST_END || t.starts_with("# --- AUTO-GAMING-RULES-END") {
            block = None;
        }
        // Правила пользователя вне AUTO-блоков не трогаем (например DOMAIN-REGEX с запятыми в шаблоне)
        if in_rules && in_auto {
            if let Some(target) = rule_target_of(t) {
                if !is_valid(&target) {
                    let fallback = match block {
                        Some("wl") if has_group(WHITELIST_GROUP_NAME) => WHITELIST_GROUP_NAME.to_string(),
                        Some("gaming") if has_group(GAMING_GROUP_NAME) => GAMING_GROUP_NAME.to_string(),
                        _ => main_fallback.clone(),
                    };
                    if is_valid(&fallback) {
                        let idx = line.rfind(&target).unwrap_or(0);
                        let new_line = format!("{}{}{}", &line[..idx], fallback, &line[idx + target.len()..]);
                        replaced.push(target);
                        out.push(new_line);
                        continue;
                    }
                }
            }
        }
        out.push(line.to_string());
    }
    let mut joined = out.join("\n");
    if yaml.ends_with('\n') {
        joined.push('\n');
    }
    (joined, replaced)
}

pub fn apply_routing(yaml: &str, cfg: &crate::config::AppConfig) -> Result<(String, usize), String> {
    let mut filters = cfg.provider_filters.clone();
    apply_routing_with_filters(yaml, cfg, &mut filters)
}

/// Очистка устаревших ipset списков при смене режимов маршрутизации (BUG-19).
#[cfg(test)]
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
