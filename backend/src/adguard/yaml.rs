use std::path::PathBuf;
use crate::config::AdGuardConfig;
use super::{agh_request, DEFAULT_ADGUARD_PORT, FALLBACK_UPSTREAMS};

/// Проверка риска циклической пересылки DNS (Loop Risk)
pub fn detect_agh_loop_risk(
    upstreams: &[String],
    router_ip: Option<&str>,
    local_dns_port: u16,
) -> (bool, Option<String>) {
    let local_port_str = format!(":{}", local_dns_port);
    let r_ip = router_ip.map(str::trim).filter(|s| !s.is_empty());

    for upstream in upstreams {
        let u = upstream.trim();
        if u.is_empty() || u.starts_with('#') {
            continue;
        }

        // Проверка loopback
        let loop_ip = format!("127.0.0.1:{local_dns_port}");
        let loop_host = format!("localhost:{local_dns_port}");
        if u.contains("127.0.0.1:53")
            || u.contains("localhost:53")
            || u.contains(&loop_ip)
            || u.contains(&loop_host)
            || u.contains("127.0.0.1:7874")
        {
            return (
                true,
                Some(format!(
                    "Upstream DNS '{u}' указывает на локальный порт роутера, что вызывает критический DNS-цикл!"
                )),
            );
        }

        // Проверка IP роутера на стандартных портах
        if let Some(rip) = r_ip {
            if (u.starts_with(rip) || u.contains(&format!("//{rip}")))
                && (u.ends_with(":53") || u.ends_with(&local_port_str) || !u.contains(':'))
            {
                return (
                    true,
                    Some(format!(
                        "Upstream DNS '{u}' указывает на IP роутера ({rip}), что вызывает зацикливание запросов!"
                    )),
                );
            }
        }
    }

    (false, None)
}

/// Очистка списка upstream_dns AdGuard Home от локальных петель; запасные резолверы — только если список опустел
pub fn clean_adguard_upstreams(upstreams: &[String], router_ip: Option<&str>) -> (Vec<String>, bool) {
    let r_ip = router_ip.map(str::trim).filter(|s| !s.is_empty());
    let mut modified = false;

    let mihomo_port = crate::routing::mihomo_dns_port_from_path(&crate::config::MihomoConfig::default().config_path);
    let loop_ip = format!("127.0.0.1:{mihomo_port}");
    let loop_host = format!("localhost:{mihomo_port}");
    let loop_suffix = format!(":{mihomo_port}");
    let mut cleaned: Vec<String> = upstreams
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| {
            if s.is_empty() {
                return false;
            }
            if s.contains(&loop_ip)
                || s.contains(&loop_host)
                || s.contains("127.0.0.1:53")
                || s.contains("localhost:53")
                || s.contains("127.0.0.1:7874")
            {
                modified = true;
                return false;
            }
            if let Some(rip) = r_ip {
                if (s.starts_with(rip) || s.contains(&format!("//{rip}")))
                    && (s.ends_with(":53") || s.ends_with(&loop_suffix) || !s.contains(':'))
                {
                    modified = true;
                    return false;
                }
            }
            true
        })
        .collect();

    // Баг 114: публичные резолверы добавляются ТОЛЬКО если после удаления петель не осталось ни одного
    // upstream. Раньше они дописывались к рабочему списку пользователя на каждом тике watchdog.
    if cleaned.is_empty() {
        cleaned.extend(FALLBACK_UPSTREAMS.iter().map(|s| s.to_string()));
        modified = true;
    }

    (cleaned, modified)
}

/// Очистка bootstrap_dns AdGuard Home от локальных петель
pub fn clean_adguard_bootstrap_dns(bootstrap: &[String], router_ip: Option<&str>) -> (Vec<String>, bool) {
    let r_ip = router_ip.map(str::trim).filter(|s| !s.is_empty());
    let mut modified = false;

    let mut cleaned: Vec<String> = bootstrap
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| {
            if s.is_empty() {
                return false;
            }
            if s.contains("127.0.0.1") || s.contains("localhost") {
                modified = true;
                return false;
            }
            if let Some(rip) = r_ip {
                if s.starts_with(rip) || s.contains(&format!("//{rip}")) {
                    modified = true;
                    return false;
                }
            }
            true
        })
        .collect();

    if cleaned.is_empty() {
        cleaned.extend(FALLBACK_UPSTREAMS.iter().map(|s| s.to_string()));
        modified = true;
    }

    (cleaned, modified)
}

/// Лечение всех найденных файлов AdGuardHome.yaml на диске
pub async fn heal_all_adguard_yaml_configs(dns_port: u16) {
    let standard = [
        "/opt/etc/AdGuardHome/AdGuardHome.yaml",
        "/opt/etc/AdGuardHome.yaml",
        "/opt/AdGuardHome/AdGuardHome.yaml",
        "/opt/root/AdGuardHome/AdGuardHome.yaml",
    ];
    for path_str in &standard {
        let p = std::path::Path::new(path_str);
        if p.is_file() {
            if let Ok(content) = tokio::fs::read_to_string(p).await {
                let (healed, changed) = sanitize_and_heal_adguard_yaml(&content, dns_port);
                if changed {
                    // Баг 115a: атомарная замена — без обрезанного конфига при обрыве
                    let _ = crate::api::atomic_write_file(p, &healed).await;
                    crate::log_i!("[ADGUARD] ✓ Конфигурационный файл {} очищен и обновлен на диске", path_str);
                }
            }
        }
    }
}

/// Автономное живое устранение DNS-петель в AdGuard Home через REST API и дисковые конфиги
pub async fn heal_adguard_live_config(
    http: &reqwest::Client,
    cfg: &AdGuardConfig,
    router_ip: Option<&str>,
) -> Result<bool, String> {
    let dns_info = match agh_request(http, cfg, reqwest::Method::GET, "/control/dns_info", None, 3).await {
        Ok(v) => v,
        Err(e) => return Err(e),
    };

    let mut current_upstreams: Vec<String> = Vec::new();
    if let Some(arr) = dns_info.get("upstream_dns").and_then(|a| a.as_array()) {
        for item in arr {
            if let Some(s) = item.as_str() {
                current_upstreams.push(s.to_string());
            }
        }
    }

    let mut current_bootstrap: Vec<String> = Vec::new();
    if let Some(arr) = dns_info.get("bootstrap_dns").and_then(|a| a.as_array()) {
        for item in arr {
            if let Some(s) = item.as_str() {
                current_bootstrap.push(s.to_string());
            }
        }
    }

    let (cleaned_upstreams, upstreams_modified) = clean_adguard_upstreams(&current_upstreams, router_ip);
    let (cleaned_bootstrap, bootstrap_modified) = clean_adguard_bootstrap_dns(&current_bootstrap, router_ip);

    if upstreams_modified || bootstrap_modified {
        let mut new_config = dns_info.clone();
        if let Some(obj) = new_config.as_object_mut() {
            if upstreams_modified {
                obj.insert(
                    "upstream_dns".to_string(),
                    serde_json::to_value(&cleaned_upstreams).unwrap_or_default(),
                );
            }
            if bootstrap_modified {
                obj.insert(
                    "bootstrap_dns".to_string(),
                    serde_json::to_value(&cleaned_bootstrap).unwrap_or_default(),
                );
            }
        }
        let _ = agh_request(
            http,
            cfg,
            reqwest::Method::POST,
            "/control/dns_config",
            Some(new_config),
            5,
        )
        .await?;
        crate::log_i!(
            "[ADGUARD] ✓ Автономное устранение DNS-петли: параметры DNS успешно очищены и обновлены через Live API"
        );
    }

    heal_all_adguard_yaml_configs(cfg.dns_port).await;

    Ok(upstreams_modified || bootstrap_modified)
}

/// Чтение DNS-порта AdGuard Home из YAML с безопасным стандартным значением DEFAULT_ADGUARD_PORT (5353).
pub fn get_adguard_port_from_yaml(yaml: &str) -> u16 {
    parse_adguard_dns_port_from_yaml(yaml).unwrap_or(DEFAULT_ADGUARD_PORT)
}

/// Поиск конфигурационного файла AdGuardHome.yaml
pub fn find_adguard_config() -> Option<PathBuf> {
    let standard = [
        "/opt/etc/AdGuardHome/AdGuardHome.yaml",
        "/opt/etc/AdGuardHome.yaml",
        "/opt/AdGuardHome/AdGuardHome.yaml",
        "/opt/root/AdGuardHome/AdGuardHome.yaml",
        "AdGuardHome.yaml",
    ];
    for p in &standard {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

/// Определение активного DNS-порта AdGuard Home из конфигурационного файла (AdGuardHome.yaml)
pub fn detect_adguard_dns_port() -> Option<u16> {
    let conf_path = find_adguard_config()?;
    let content = std::fs::read_to_string(&conf_path).ok()?;
    parse_adguard_dns_port_from_yaml(&content)
}

/// Извлечение порта DNS из содержимого YAML AdGuard Home
pub fn parse_adguard_dns_port_from_yaml(yaml: &str) -> Option<u16> {
    let mut in_dns = false;
    for line in yaml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            continue;
        }
        if trimmed == "dns:" {
            in_dns = true;
            continue;
        }
        if in_dns {
            if !line.starts_with(' ') && !line.starts_with('\t') && line.contains(':') {
                break;
            }
            if trimmed.starts_with("port:") {
                let parts: Vec<&str> = trimmed.split(':').collect();
                if parts.len() >= 2 {
                    if let Ok(port) = parts[1].trim().parse::<u16>() {
                        if port > 0 {
                            return Some(port);
                        }
                    }
                }
            }
        }
    }
    None
}

/// Генерация автономного скрипта инициализации /opt/etc/init.d/S99adguardhome

/// Генерация готовой стандартной конфигурации AdGuardHome.yaml
/// Задает schema_version: 29 для полного пропуска мастера первичной настройки (setup wizard)
pub fn generate_default_adguard_yaml(dns_port: u16, http_port: u16) -> String {
    format!(
r#"bind_host: 0.0.0.0
bind_port: {http_port}
auth_attempts: 5
block_auth_min: 15
http_proxy: "http://127.0.0.1:7890"
language: ru
rlimit_nofile: 0
dns:
  bind_hosts:
    - 0.0.0.0
  port: {dns_port}
  statistics_interval: 1
  querylog_enabled: true
  querylog_file_enabled: true
  querylog_interval: 24h
  querylog_size_memory: 1000
  anonymize_client_ip: false
  protection_enabled: true
  blocking_mode: default
  blocking_ipv4: ""
  blocking_ipv6: ""
  blocked_response_ttl: 10
  parental_block_host: ""
  safebrowsing_block_host: ""
  ratelimit: 0
  ratelimit_whitelist: []
  refuse_any: true
  upstream_dns:
    - 77.88.8.8
    - 77.88.8.1
  upstream_dns_file: ""
  bootstrap_dns:
    - 77.88.8.8
    - 77.88.8.1
  fallback_dns:
    - 77.88.8.8
  all_servers: false
  fastest_addr: true
  fastest_timeout: 1s
  use_http3_upstreams: false
  use_dns64: false
  dns64_prefixes: []
  edns_client_subnet:
    custom_ip: ""
    enabled: false
    use_custom: false
  max_goroutines: 300
  handle_ddr: true
  ipset: []
  ipset_file: ""
  filtering_enabled: true
  filters_update_interval: 24
  parental_enabled: false
  safesearch_enabled: false
  safebrowsing_enabled: false
  safebrowsing_cache_size: 1048576
  safesearch_cache_size: 1048576
  parental_cache_size: 1048576
  cache_size: 4194304
  cache_time: 30
  rewrites:
    - domain: probe.test
      answer: 127.0.0.1
  blocked_services: []
  upstream_timeout: 10s
  private_networks: []
  use_private_ptr_resolvers: true
  local_ptr_upstreams: []
tls:
  enabled: false
filters:
  - enabled: true
    url: https://adguardteam.github.io/HostlistsRegistry/assets/filter_1.txt
    name: AdGuard Base Filter
    id: 1
  - enabled: true
    url: https://filters.adtidy.org/extension/chromium/filters/2.txt
    name: AdGuard Russian Filter
    id: 1792272695
  - enabled: true
    url: https://adguardteam.github.io/HostlistsRegistry/assets/filter_48.txt
    name: HaGeZi Multi Pro
    id: 1792272696
whitelist_filters: []
user_rules:
  - "||chrome.cloudflare-dns.com^"
  - "||mozilla.cloudflare-dns.com^"
  - "||cloudflare-dns.com^"
  - "||dns.google^"
  - "||yabs.yandex.ru^"
  - "||ads.vk.com^"
dhcp:
  enabled: false
clients:
  runtime_sources:
    whois: true
    arp: true
    rdns: true
    dhcp: true
    hosts: true
  persistent: []
log_compress: false
log_localtime: false
log_max_backups: 0
log_max_size: 100
log_max_age: 3
log_file: /opt/var/log/adguardhome.log
verbose: false
os:
  group: ""
  user: ""
  rlimit_nofile: 0
schema_version: 29
"#
    )
}

/// Автоматическое исправление/миграция конфигурации AdGuard Home:
/// 1. Переключение с конфликтующего порта 5353 (занят avahi-daemon на KeeneticOS) на 5354.
/// 2. Исключение дублирования 127.0.0.1 в bind_hosts (оставляем только 0.0.0.0 для dual-stack).
/// 3. Настройка http_proxy ("http://127.0.0.1:7890") для мгновенного обновления фильтров через локальный прокси.
/// 4. Удаление 127.0.0.1:1053 из upstream_dns для устранения бесконечного зацикливания с Mihomo и добавление быстрых апстримов (77.88.8.8, 77.88.8.1) при пустом списке.
/// 5. Добавление rewrite для probe.test -> 127.0.0.1 для мгновенного ответа на healthcheck.
/// 6. Защита от DoH-обхода в браузерах (chrome.cloudflare-dns.com, mozilla.cloudflare-dns.com) и блокировка рекламы в user_rules.
/// 7. Внедрение AdGuard Russian Filter и HaGeZi Multi Pro в список подписок filters.
pub fn sanitize_and_heal_adguard_yaml(content: &str, target_port: u16) -> (String, bool) {
    let mixed_port = crate::routing::mihomo_mixed_port_from_path(&crate::config::MihomoConfig::default().config_path);
    sanitize_and_heal_adguard_yaml_with_proxy_port(content, target_port, mixed_port)
}

pub fn sanitize_and_heal_adguard_yaml_with_proxy_port(
    content: &str,
    target_port: u16,
    mixed_port: u16,
) -> (String, bool) {
    let mut changed = false;
    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();

    // 1. Проверяем и правим порт если он 5353 или 53 (когда 53 занят)
    for line in &mut lines {
        let trimmed = line.trim();
        if trimmed == "port: 5353" || (target_port != 53 && trimmed == "port: 53") {
            *line = format!("  port: {}", target_port);
            changed = true;
        }
    }

    // 2. Убираем дублирование "- 127.0.0.1" из секции bind_hosts, если есть "- 0.0.0.0"
    let mut in_dns = false;
    let mut in_bind_hosts = false;
    let mut has_zero_bind = false;
    let mut has_loopback_bind = false;

    for line in &lines {
        let trimmed = line.trim();
        if trimmed == "dns:" {
            in_dns = true;
            continue;
        }
        if in_dns && (trimmed == "bind_hosts:" || trimmed == "bind_hosts: []") {
            in_bind_hosts = true;
            continue;
        }
        if in_bind_hosts {
            if !line.starts_with("    -") && !line.starts_with("\t\t-") {
                if !trimmed.starts_with('-') {
                    in_bind_hosts = false;
                }
            } else {
                if trimmed == "- 0.0.0.0" {
                    has_zero_bind = true;
                } else if trimmed == "- 127.0.0.1" {
                    has_loopback_bind = true;
                }
            }
        }
        if in_dns && !line.starts_with(' ') && !line.starts_with('\t') && line.contains(':') {
            in_dns = false;
        }
    }

    if has_zero_bind && has_loopback_bind {
        let mut in_dns_sec = false;
        let mut in_bind_hosts_sec = false;
        let mut filtered = Vec::with_capacity(lines.len());
        for l in lines {
            let t = l.trim();
            if t == "dns:" {
                in_dns_sec = true;
                in_bind_hosts_sec = false;
            } else if in_dns_sec && !l.starts_with(' ') && !t.is_empty() && !t.starts_with('#') {
                in_dns_sec = false;
                in_bind_hosts_sec = false;
            } else if in_dns_sec && t == "bind_hosts:" {
                in_bind_hosts_sec = true;
            } else if in_bind_hosts_sec && !t.is_empty() && !t.starts_with('-') && !t.starts_with('#') {
                in_bind_hosts_sec = false;
            }
            if in_bind_hosts_sec && t == "- 127.0.0.1" {
                changed = true;
                continue;
            }
            filtered.push(l);
        }
        lines = filtered;
    }

    // 3. Настройка http_proxy для скачивания фильтров через Mihomo (обход блокировок GitHub/CDN)
    let proxy_port = if mixed_port > 0 { mixed_port } else { 7890 };
    let proxy_line = format!("http_proxy: \"http://127.0.0.1:{proxy_port}\"");
    let mut has_http_proxy = false;
    for line in &mut lines {
        let trimmed = line.trim();
        if trimmed.starts_with("http_proxy:") {
            has_http_proxy = true;
            if trimmed == "http_proxy: \"\"" || trimmed == "http_proxy:" || trimmed == "http_proxy: ''" {
                *line = proxy_line.clone();
                changed = true;
            }
        }
    }
    if !has_http_proxy {
        lines.insert(0, proxy_line);
        changed = true;
    }

    // 4. Устранение циклического DNS-лупа (Mihomo nameserver <-> AdGuard upstream):
    // Удаляем 127.0.0.1:<mihomo_port> и 127.0.0.1:53 из upstream_dns, гарантируя прямые чистые резолверы.
    let mihomo_port = crate::routing::mihomo_dns_port_from_path(&crate::config::MihomoConfig::default().config_path);
    let loop_ip_line = format!("- 127.0.0.1:{mihomo_port}");
    let loop_host_line = format!("- localhost:{mihomo_port}");
    lines.retain(|l| {
        let t = l.trim();
        if t == loop_ip_line || t == "- 127.0.0.1:53" || t == loop_host_line || t == "- localhost:53" {
            changed = true;
            false
        } else {
            true
        }
    });

    // Баг 114: запасные резолверы вставляются, только если блок upstream_dns остался пустым.
    if let Some(pos) = lines.iter().position(|l| l.trim() == "upstream_dns:") {
        let has_entries = lines[pos + 1..]
            .iter()
            .take_while(|l| l.trim().is_empty() || l.trim().starts_with("- ") || l.trim().starts_with('#'))
            .any(|l| l.trim().starts_with("- "));
        if !has_entries {
            for (i, up) in FALLBACK_UPSTREAMS.iter().enumerate() {
                lines.insert(pos + 1 + i, format!("    - {up}"));
            }
            changed = true;
        }
    }

    // 5. Добавляем probe.test в rewrites если его нет
    let content_with_upstreams = lines.join("\n");
    if !content_with_upstreams.contains("probe.test") {
        let mut new_lines = Vec::with_capacity(lines.len() + 4);
        let mut added_rewrite = false;
        for line in lines {
            let trimmed = line.trim();
            if (trimmed == "rewrites: []" || trimmed == "rewrites:") && !added_rewrite {
                new_lines.push("  rewrites:".to_string());
                new_lines.push("    - domain: probe.test".to_string());
                new_lines.push("      answer: 127.0.0.1".to_string());
                added_rewrite = true;
                changed = true;
            } else {
                new_lines.push(line);
            }
        }
        lines = new_lines;
    }

    // 6. Защита от DoH-обхода браузеров (Chrome, Firefox, Edge) и явная блокировка рекламы в user_rules
    let content_rules = lines.join("\n");
    if !content_rules.contains("chrome.cloudflare-dns.com") {
        let mut new_lines = Vec::with_capacity(lines.len() + 8);
        let mut added_rules = false;
        for line in lines {
            let trimmed = line.trim();
            if (trimmed == "user_rules: []" || trimmed == "user_rules:") && !added_rules {
                new_lines.push("user_rules:".to_string());
                new_lines.push("  - \"||chrome.cloudflare-dns.com^\"".to_string());
                new_lines.push("  - \"||mozilla.cloudflare-dns.com^\"".to_string());
                new_lines.push("  - \"||cloudflare-dns.com^\"".to_string());
                new_lines.push("  - \"||dns.google^\"".to_string());
                new_lines.push("  - \"||yabs.yandex.ru^\"".to_string());
                new_lines.push("  - \"||ads.vk.com^\"".to_string());
                added_rules = true;
                changed = true;
            } else {
                new_lines.push(line);
            }
        }
        lines = new_lines;
    }

    // 7. Автоматическое добавление русской подписки и HaGeZi Multi Pro в filters
    let content_filters = lines.join("\n");
    let needs_russian = !content_filters.contains("filters/2.txt") && !content_filters.contains("AdGuard Russian Filter");
    let needs_hagezi = !content_filters.contains("filter_48.txt") && !content_filters.contains("HaGeZi Multi Pro");

    if needs_russian || needs_hagezi {
        let mut new_lines = Vec::with_capacity(lines.len() + 16);
        let mut injected_filters = false;
        for line in lines {
            let trimmed = line.trim();
            if (trimmed == "filters:" || trimmed == "filters: []") && !injected_filters {
                new_lines.push("filters:".to_string());
                if needs_russian {
                    new_lines.push("  - enabled: true".to_string());
                    new_lines.push("    url: https://filters.adtidy.org/extension/chromium/filters/2.txt".to_string());
                    new_lines.push("    name: AdGuard Russian Filter".to_string());
                    new_lines.push("    id: 1792272695".to_string());
                    changed = true;
                }
                if needs_hagezi {
                    new_lines.push("  - enabled: true".to_string());
                    new_lines.push("    url: https://adguardteam.github.io/HostlistsRegistry/assets/filter_48.txt".to_string());
                    new_lines.push("    name: HaGeZi Multi Pro".to_string());
                    new_lines.push("    id: 1792272696".to_string());
                    changed = true;
                }
                injected_filters = true;
            } else {
                new_lines.push(line);
            }
        }
        if !injected_filters {
            new_lines.push("filters:".to_string());
            if needs_russian {
                new_lines.push("  - enabled: true".to_string());
                new_lines.push("    url: https://filters.adtidy.org/extension/chromium/filters/2.txt".to_string());
                new_lines.push("    name: AdGuard Russian Filter".to_string());
                new_lines.push("    id: 1792272695".to_string());
                changed = true;
            }
            if needs_hagezi {
                new_lines.push("  - enabled: true".to_string());
                new_lines.push("    url: https://adguardteam.github.io/HostlistsRegistry/assets/filter_48.txt".to_string());
                new_lines.push("    name: HaGeZi Multi Pro".to_string());
                new_lines.push("    id: 1792272696".to_string());
                changed = true;
            }
        }
        lines = new_lines;
    }

    let mut result = lines.join("\n");
    if content.ends_with('\n') && !result.ends_with('\n') {
        result.push('\n');
    }
    let actually_changed = changed && result != content;
    (result, actually_changed)
}

/// Гарантированное создание стандартного файла конфигурации, если он еще не существует
pub fn ensure_default_adguard_config(dns_port: u16, http_port: u16) -> PathBuf {
    let conf_path = find_adguard_config().unwrap_or_else(|| PathBuf::from("/opt/etc/AdGuardHome/AdGuardHome.yaml"));
    if let Some(parent) = conf_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if !conf_path.is_file() {
        let content = generate_default_adguard_yaml(dns_port, http_port);
        let _ = crate::transaction::sync_atomic_write_or_remove(&conf_path, Some(&content));
        crate::log_i!("Создана стандартная конфигурация AdGuard Home: {:?}", conf_path);
    } else if let Ok(existing) = std::fs::read_to_string(&conf_path) {
        let (healed, changed) = sanitize_and_heal_adguard_yaml(&existing, dns_port);
        if changed {
            let _ = crate::transaction::sync_atomic_write_or_remove(&conf_path, Some(&healed));
            crate::log_i!("Конфигурация AdGuard Home автоматически оптимизирована и самоисцелена: {:?}", conf_path);
        }
    }
    conf_path
}
