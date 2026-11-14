use chrono::Local;
use serde::{Deserialize, Serialize};
use crate::mihomo::{self, Server};
use crate::AppState;
use super::*;

// ============================================================================
// Детектор белых списков (Whitelist Detector) и Классификатор [БС]
// ============================================================================

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WhitelistSuitability {
    pub is_suitable_bs: bool,
    pub score: u32,
    pub reasons: Vec<String>,
}

/// Классификатор серверов на пригодность в роли [БС] / Моста (Bridge Analyzer)
pub fn classify_server_whitelist_suitability(
    server: &Server,
    provider_yaml_content_opt: Option<&str>,
    _is_isolation_active: bool,
) -> WhitelistSuitability {
    let mut score = 0u32;
    let mut reasons = Vec::new();

    let name_lower = server.name.to_lowercase();
    let id_lower = server.id.to_lowercase();
    let host_lower = server.host.to_lowercase();

    // 1. Маркеры в названии или ID (используем единый has_bs_tag + маркеры мостов)
    if crate::whitelist_tags::has_bs_tag(&server.name) || crate::whitelist_tags::has_bs_tag(&server.id) {
        score += 40;
        reasons.push("Маркер БС в названии/ID".to_string());
    } else {
        let bs_bridge_markers = [
            "обход", "мост", "bridge", "cascade", "каскад", "relay", "вход рф", "вход-рф", "вход_рф",
        ];
        for marker in &bs_bridge_markers {
            if name_lower.contains(marker) || id_lower.contains(marker) {
                score += 40;
                reasons.push(format!("Маркер БС в названии/ID: '{marker}'"));
                break;
            }
        }
    }

    // 2. Российские домены хоста: .ru, .рф, .su
    if host_lower.ends_with(".ru") || host_lower.ends_with(".рф") || host_lower.ends_with(".su")
        || host_lower.contains(".ru:") || host_lower.contains(".рф:") || host_lower.contains(".su:")
    {
        score += 30;
        reasons.push("Российская доменная зона хоста (.ru/.рф/.su)".to_string());
    }

    // 3. Российские провайдеры / ASN / Хостинг-провайдеры в хосте или имени
    let ru_infra_keywords = [
        "selectel", "yandex", "ya.ru", "vk.com", "mail.ru", "rostelecom", "rt.ru",
        "mts", "beeline", "megafon", "vdsina", "timeweb", "reg.ru", "ru-center", "nic.ru",
        "sber", "tbank", "tinkoff", "spacecore", "fastvps", "aeza",
    ];
    for kw in &ru_infra_keywords {
        if host_lower.contains(kw) || name_lower.contains(kw) || id_lower.contains(kw) {
            score += 25;
            reasons.push(format!("Инфраструктура/ASN РФ: '{kw}'"));
            break;
        }
    }

    // 4. Российские диапазоны IP-адресов
    let host_ip_opt = server.host.parse::<std::net::IpAddr>().ok()
        .or_else(|| server.resolved_ip.as_deref().and_then(|ip| ip.parse::<std::net::IpAddr>().ok()));

    if let Some(std::net::IpAddr::V4(ipv4)) = host_ip_opt {
        let oct = ipv4.octets();
        // Диапазоны крупных российских провайдеров и ДЦ:
        // Selectel (188.93.16.0/20, 95.213.128.0/18, 92.53.64.0/18, 178.62.192.0/18)
        // Yandex (87.250.250.0/24, 77.88.0.0/18, 93.158.134.0/24, 213.180.193.0/24)
        // VK / Mail.ru (95.163.0.0/16, 217.69.128.0/18, 128.140.168.0/21)
        // Rostelecom (178.46.0.0/15, 188.162.0.0/16, 212.164.0.0/16, 85.26.0.0/16)
        // MTS / Beeline / Megafon (213.87.0.0/16, 217.118.64.0/19, 83.149.0.0/16)
        let is_ru_cidr = match oct[0] {
            188 => oct[1] == 93 || oct[1] == 162 || oct[1] == 225,
            95 => oct[1] == 213 || oct[1] == 163,
            92 => oct[1] == 53,
            77 => oct[1] == 88,
            87 => oct[1] == 250,
            93 => oct[1] == 158,
            213 => oct[1] == 180 || oct[1] == 87,
            217 => oct[1] == 69 || oct[1] == 118,
            178 => oct[1] == 62 || (oct[1] >= 44 && oct[1] <= 47),
            212 => oct[1] == 164,
            85 => oct[1] == 26,
            83 => oct[1] == 149,
            _ => false,
        };

        if is_ru_cidr {
            score += 35;
            reasons.push(format!("Российский диапазон IP-адресов ({}.{}.*.*)", oct[0], oct[1]));
        }
    }

    // 5. SNI из белых списков / Отечественные SNI в YAML конфигурации узла
    if let Some(content) = provider_yaml_content_opt {
        // Ищем упоминания узла в yaml
        let escaped_name = regex_lite::escape(&server.name);
        if let Ok(re) = regex_lite::Regex::new(&format!(r#"(?i)name:\s*['"]?{}['"]?"#, escaped_name)) {
            if let Some(m) = re.find(content) {
                let start = m.start();
                let snippet = &content[start..std::cmp::min(start + 1500, content.len())];
                for sni_cand in ["sni:", "server-name:"] {
                    if let Some(sni_pos) = snippet.to_lowercase().find(sni_cand) {
                        let line = snippet[sni_pos..].lines().next().unwrap_or_default();
                        for domestic in ["gosuslugi.ru", "sberbank.ru", "yandex.ru", "vk.com", "mail.ru", "ozon.ru", "wildberries.ru", "tinkoff.ru", "mos.ru"] {
                            if line.to_lowercase().contains(domestic) {
                                score += 35;
                                reasons.push(format!("Domestic Whitelist SNI ('{domestic}')"));
                                break;
                            }
                        }
                    }
                }
            }
        }
    }

    // 6. Российские маркеры локации в имени ноды (Москва, Спб, RU, Russia)
    if is_ru_node(&server.name, &server.id) {
        score += 20;
        reasons.push("Российская локация узла".to_string());
    }

    let is_suitable = score >= 35;
    WhitelistSuitability {
        is_suitable_bs: is_suitable,
        score,
        reasons,
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DetectedProviderBsInfo {
    pub provider: String,
    pub bs_nodes_count: usize,
    pub total_nodes_count: usize,
    pub is_bs_provider: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DetectedBsResult {
    pub detected_bs_servers: Vec<String>,
    pub detected_bs_providers: Vec<String>,
    pub suitability_map: std::collections::BTreeMap<String, WhitelistSuitability>,
    pub provider_stats: Vec<DetectedProviderBsInfo>,
    pub suggested_monitored_provider: Option<String>,
    pub suggested_target_bs_provider: Option<String>,
}

/// Комплексный поиск и классификация серверов и провайдеров белых списков
pub async fn detect_whitelist_servers_and_providers(state: &AppState) -> Result<DetectedBsResult, String> {
    let cfg = state.config.read().await.clone();
    let vkturn_ready = state.vkturn.rt.read().await.tunnel_ready();
    let servers = mihomo::get_servers(&state.http, &cfg, &cfg.failover.priority_chain).await?;

    // Читаем содержимое локальных файлов провайдеров для анализа SNI
    let prov_dir = std::path::Path::new(&cfg.mihomo.config_path)
        .parent()
        .map(|p| p.join("providers"))
        .unwrap_or_else(|| std::path::PathBuf::from("/opt/etc/mihomo/providers"));

    let mut provider_yaml_cache: std::collections::HashMap<String, String> = std::collections::HashMap::new();

    let mut suitability_map = std::collections::BTreeMap::new();
    let mut detected_bs_servers = Vec::new();
    let mut provider_nodes: std::collections::HashMap<String, (usize, usize)> = std::collections::HashMap::new(); // (bs_nodes, total_nodes)

    for s in &servers {
        if s.is_pool || s.id == "Fastest" || s.id == "Fallback" || s.id == "DIRECT" || s.id == "REJECT" || s.id == "GLOBAL" || s.id.starts_with("AUTO-") {
            continue;
        }
        if !vkturn_ready
            && (s.id == crate::vkturn::PROXY_NAME
                || s.provider.as_deref() == Some(crate::vkturn::PROVIDER_ID))
        {
            continue;
        }

        let prov_name = s.provider.as_deref().unwrap_or("").to_string();
        let yaml_content = if !prov_name.is_empty() {
            if let Some(c) = provider_yaml_cache.get(&prov_name) {
                Some(c.as_str())
            } else {
                let path = prov_dir.join(format!("{prov_name}.yaml"));
                if let Ok(c) = tokio::fs::read_to_string(&path).await {
                    provider_yaml_cache.insert(prov_name.clone(), c);
                    provider_yaml_cache.get(&prov_name).map(|s| s.as_str())
                } else {
                    None
                }
            }
        } else {
            None
        };

        let suitability = classify_server_whitelist_suitability(s, yaml_content, false);
        let is_bs = suitability.is_suitable_bs;

        if is_bs {
            detected_bs_servers.push(s.id.clone());
        }

        if !prov_name.is_empty() {
            let entry = provider_nodes.entry(prov_name.clone()).or_insert((0, 0));
            entry.1 += 1;
            if is_bs {
                entry.0 += 1;
            }
        }

        suitability_map.insert(s.id.clone(), suitability);
    }

    let mut detected_bs_providers = Vec::new();
    let mut provider_stats = Vec::new();
    let mut regular_providers = Vec::new();

    for (pname, (bs_cnt, tot_cnt)) in provider_nodes {
        let is_bs_prov = if tot_cnt > 0 {
            (bs_cnt as f64 / tot_cnt as f64) >= 0.5 || bs_cnt >= 3
        } else {
            false
        };

        if is_bs_prov {
            detected_bs_providers.push(pname.clone());
        } else {
            regular_providers.push(pname.clone());
        }

        provider_stats.push(DetectedProviderBsInfo {
            provider: pname,
            bs_nodes_count: bs_cnt,
            total_nodes_count: tot_cnt,
            is_bs_provider: is_bs_prov,
        });
    }

    provider_stats.sort_by(|a, b| a.provider.cmp(&b.provider));
    detected_bs_providers.sort();
    detected_bs_servers.sort();

    let suggested_monitored_provider = regular_providers.first().cloned();
    let suggested_target_bs_provider = detected_bs_providers.first().cloned();

    Ok(DetectedBsResult {
        detected_bs_servers,
        detected_bs_providers,
        suitability_map,
        provider_stats,
        suggested_monitored_provider,
        suggested_target_bs_provider,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WhitelistDetectorState {
    Normal,
    WhitelistDetected,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WhitelistDetectorStatus {
    pub state: WhitelistDetectorState,
    pub monitored_provider: String,
    pub total_nodes: usize,
    #[serde(default)]
    pub monitored_total_nodes: usize,
    pub offline_nodes: usize,
    pub online_nodes: usize,
    pub offline_percent: u32,
    pub last_checked: Option<String>,
    pub last_event: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

static DETECTOR_STATE: tokio::sync::Mutex<WhitelistDetectorState> = tokio::sync::Mutex::const_new(WhitelistDetectorState::Normal);
static DETECTOR_LAST_STATUS: tokio::sync::Mutex<Option<WhitelistDetectorStatus>> = tokio::sync::Mutex::const_new(None);

/// Путь к файлу состояния детектора БС (рядом с quarantine.json, Баг 105)
pub fn detector_state_path() -> std::path::PathBuf {
    #[cfg(target_os = "linux")]
    {
        std::path::PathBuf::from("/opt/etc/xkeen-route/detector_state.json")
    }
    #[cfg(not(target_os = "linux"))]
    {
        std::env::temp_dir().join("xkeen-route.detector_state.json")
    }
}

/// Атомарное сохранение состояния детектора БС на диск (Баг 105)
pub fn persist_detector_state(path: &std::path::Path, state: WhitelistDetectorState) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| format!("create_dir_all: {e}"))?;
        }
    }
    let json = serde_json::to_string(&state).map_err(|e| format!("serialize detector state: {e}"))?;
    let tmp = path.with_extension(format!("json.tmp.{}", std::process::id()));
    std::fs::write(&tmp, json).map_err(|e| format!("write tmp detector state: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("rename detector state: {e}"))?;
    Ok(())
}

/// Загрузка состояния детектора БС с диска; при повреждении возвращает Normal и пишет в лог (Баг 105)
pub fn load_detector_state(path: &std::path::Path) -> WhitelistDetectorState {
    match std::fs::read_to_string(path) {
        Ok(content) => match serde_json::from_str::<WhitelistDetectorState>(content.trim()) {
            Ok(state) => state,
            Err(e) => {
                crate::log_w!("[DETECTOR] Повреждён файл состояния детектора БС {}: {e}; сброс в Normal", path.display());
                WhitelistDetectorState::Normal
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => WhitelistDetectorState::Normal,
        Err(e) => {
            crate::log_w!("[DETECTOR] Ошибка чтения состояния детектора БС {}: {e}; сброс в Normal", path.display());
            WhitelistDetectorState::Normal
        }
    }
}

/// Восстановление состояния детектора БС при старте панели до первого скана (Баг 105)
pub async fn init_detector_state_from_disk() {
    let loaded = load_detector_state(&detector_state_path());
    *DETECTOR_STATE.lock().await = loaded;
}

pub async fn get_detector_state() -> WhitelistDetectorState {
    *DETECTOR_STATE.lock().await
}

pub async fn set_detector_state(state: WhitelistDetectorState) {
    *DETECTOR_STATE.lock().await = state;
    if let Err(e) = persist_detector_state(&detector_state_path(), state) {
        crate::log_w!("[DETECTOR] Не удалось сохранить состояние детектора БС: {e}");
    }
}

pub async fn get_whitelist_detector_status() -> WhitelistDetectorStatus {
    if let Some(status) = DETECTOR_LAST_STATUS.lock().await.clone() {
        status
    } else {
        WhitelistDetectorStatus {
            state: get_detector_state().await,
            monitored_provider: String::new(),
            total_nodes: 0,
            monitored_total_nodes: 0,
            offline_nodes: 0,
            online_nodes: 0,
            offline_percent: 0,
            last_checked: None,
            last_event: None,
            warning: None,
        }
    }
}

/// Проверка, является ли нода российской (для отсева RU нод из контролируемой группы зарубежных узлов)
pub fn is_ru_node(node_name: &str, node_id: &str) -> bool {
    let lower_name = node_name.to_lowercase();
    let lower_id = node_id.to_lowercase();

    let check = |s: &str| -> bool {
        s.contains("росси")
            || s.contains("russia")
            || s.contains(" ru ")
            || s.contains("[ru]")
            || s.contains("(ru)")
            || s.contains(" ru-")
            || s.contains(" ru_")
            || s.contains("-ru-")
            || s.contains("-ru_")
            || s.contains("_ru-")
            || s.contains("_ru_")
            || s.starts_with("ru ")
            || s.starts_with("ru-")
            || s.starts_with("ru_")
            || s.ends_with(" ru")
            || s.ends_with("-ru")
            || s.ends_with("_ru")
            || s.contains("🇷🇺")
            || s.contains("москва")
            || s.contains("moscow")
            || s.contains("санкт-петербург")
            || s.contains("питер")
            || s.contains("спб")
            || s.contains("spb")
    };

    check(&lower_name) || check(&lower_id)
}

/// Опрос контролируемых узлов детектора белых списков
pub async fn check_whitelist_detector_nodes(
    http: &reqwest::Client,
    cfg: &crate::config::AppConfig,
    servers: &[Server],
) -> Result<(usize, usize, usize, u32, std::collections::BTreeMap<String, i64>, usize), String> {
    let det_cfg = &cfg.failover.whitelist_detector;

    // Отбираем узлы контролируемого провайдера (или все обычные VPS, если monitored_provider пуст)
    let candidate_nodes: Vec<&Server> = servers
        .iter()
        .filter(|s| {
            if s.is_pool || s.id == "Fastest" || s.id == "Fallback" || s.id == "DIRECT" || s.id == "REJECT" || s.id == "GLOBAL" || s.id.starts_with("AUTO-") {
                return false;
            }
            // Не должен быть сервером белых списков
            if is_whitelist_server_full(&s.name, &s.id, s.provider.as_deref(), cfg) {
                return false;
            }
            // Если задан конкретный провайдер подписки, фильтруем по нему
            if !det_cfg.monitored_provider.is_empty() {
                if let Some(ref p) = s.provider {
                    if !p.eq_ignore_ascii_case(&det_cfg.monitored_provider) {
                        return false;
                    }
                } else {
                    return false;
                }
            }
            // Отсев RU нод
            if det_cfg.exclude_ru_nodes && is_ru_node(&s.name, &s.id) {
                return false;
            }
            true
        })
        .collect();

    let total = candidate_nodes.len();
    if total == 0 {
        return Err("Нет подходящих зарубежных узлов для мониторинга детектора белых списков".to_string());
    }

    // Если det_cfg.sample_size == 0: проверяем все подходящие узлы без ограничений.
    // Если det_cfg.sample_size > 0 и узлов больше чем sample_size, делаем случайную выборку без смещения.
    let sample_seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x517cc1b727220a95);
    let sampled_candidates: Vec<&Server> =
        sample_candidates(&candidate_nodes, det_cfg.sample_size, sample_seed);

    let sample_total = sampled_candidates.len();
    let ids: Vec<String> = sampled_candidates.iter().map(|s| s.id.clone()).collect();
    let max_ping = det_cfg.max_acceptable_ping_ms as i64;
    // Таймаут 1500мс для быстрого сканирования за 2-3 секунды
    let ping_timeout = max_ping.clamp(1000, 1500) as u64;

    let pings = mihomo::ping_all(http, cfg, &ids, ping_timeout).await;

    let ping_results: Vec<i64> = ids
        .iter()
        .map(|id| pings.get(id).copied().unwrap_or(-1))
        .collect();
    let (offline, slow, ok) = classify_pings(&ping_results, max_ping);
    let online = slow + ok;

    let offline_pct = if sample_total > 0 {
        ((offline as f64 / sample_total as f64) * 100.0).round() as u32
    } else {
        0
    };

    Ok((sample_total, offline, online, offline_pct, pings, total))
}

pub(crate) fn classify_pings(results: &[i64], max_ping: i64) -> (usize, usize, usize) {
    let mut offline = 0;
    let mut slow = 0;
    let mut ok = 0;
    for &p in results {
        if p <= 0 {
            offline += 1;
        } else if p > max_ping {
            slow += 1;
        } else {
            ok += 1;
        }
    }
    (offline, slow, ok)
}

pub(crate) fn sample_candidates<T: Clone>(items: &[T], sample_size: usize, seed: u64) -> Vec<T> {
    if sample_size == 0 || items.len() <= sample_size {
        return items.to_vec();
    }
    let mut indices: Vec<usize> = (0..items.len()).collect();
    // Deterministic Fisher-Yates shuffle seeded by caller (SplitMix64)
    let mut state = seed.wrapping_add(0x9e3779b97f4a7c15);
    for i in (1..indices.len()).rev() {
        state = state.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^= z >> 31;
        let j = (z as usize) % (i + 1);
        indices.swap(i, j);
    }
    indices.truncate(sample_size);
    indices.into_iter().map(|i| items[i].clone()).collect()
}

/// Выполняет единичную проверку детектора белых списков и, если порог превышен, переключает режим
pub async fn evaluate_and_apply_whitelist_detector(
    state: &AppState,
    cfg: &crate::config::AppConfig,
    servers: &[Server],
    _trigger_reason: &str,
) -> Result<Option<String>, String> {
    let det_cfg = &cfg.failover.whitelist_detector;

    // Проверяем наличие предупреждений о провайдере
    let mut detector_warning = None;
    if !det_cfg.monitored_provider.is_empty() {
        if cfg.disabled_providers.iter().any(|dp| dp.eq_ignore_ascii_case(&det_cfg.monitored_provider)) {
            detector_warning = Some(format!("Контролируемый провайдер '{}' отключён в настройках", det_cfg.monitored_provider));
        }
    }

    // Всегда опрашиваем узлы для актуализации метрик DETECTOR_LAST_STATUS
    let check_res = check_whitelist_detector_nodes(&state.http, cfg, servers).await;
    let (sample_total, offline, online, offline_pct, pings, total_candidates) = match check_res {
        Ok(res) => res,
        Err(e) => {
            let now_str = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
            let warn = detector_warning.unwrap_or_else(|| e.clone());
            let current_state = if det_cfg.enabled { get_detector_state().await } else { WhitelistDetectorState::Normal };
            *DETECTOR_LAST_STATUS.lock().await = Some(WhitelistDetectorStatus {
                state: current_state,
                monitored_provider: det_cfg.monitored_provider.clone(),
                total_nodes: 0,
                monitored_total_nodes: 0,
                offline_nodes: 0,
                online_nodes: 0,
                offline_percent: 0,
                last_checked: Some(now_str),
                last_event: None,
                warning: Some(warn),
            });
            return Ok(None);
        }
    };

    if sample_total == 0 && detector_warning.is_none() {
        detector_warning = Some("0 доступных узлов для мониторинга".to_string());
    }

    let now_str = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let current_state = get_detector_state().await;

    // Если детектор выключен, сохраняем метрики, поддерживаем Normal (или текущее) состояние и выходим без переключений
    if !det_cfg.enabled {
        *DETECTOR_LAST_STATUS.lock().await = Some(WhitelistDetectorStatus {
            state: current_state,
            monitored_provider: det_cfg.monitored_provider.clone(),
            total_nodes: sample_total,
            monitored_total_nodes: total_candidates,
            offline_nodes: offline,
            online_nodes: online,
            offline_percent: offline_pct,
            last_checked: Some(now_str),
            last_event: None,
            warning: detector_warning,
        });
        return Ok(None);
    }

    let is_trigger_hit = match det_cfg.threshold_mode.as_str() {
        "count" => offline >= det_cfg.trigger_offline_count,
        _ => offline_pct >= det_cfg.trigger_offline_percent,
    };

    let is_exit_hit = match det_cfg.threshold_mode.as_str() {
        "count" => offline <= det_cfg.exit_offline_count,
        _ => offline_pct <= det_cfg.exit_offline_percent,
    };

    let current_state = match get_detector_state().await { s => s };
    let vkturn_ready = state.vkturn.rt.read().await.tunnel_ready();

    match current_state {
        WhitelistDetectorState::Normal => {
            if is_trigger_hit {
                // Порог пробит! Переключаем в режим WHITELIST_DETECTED
                set_detector_state(WhitelistDetectorState::WhitelistDetected).await;

                // Сохраняем активный сервер как last_good_regular_vps, если он обычный VPS
                let active = servers.iter().find(|s| s.is_active && s.id != "Fastest" && s.id != "Fallback");
                let mut last_good = cfg.failover.last_good_regular_vps.clone();
                if let Some(act) = active {
                    if !is_whitelist_server_full(&act.name, &act.id, act.provider.as_deref(), cfg) {
                        last_good = Some(act.id.clone());
                    }
                }
                if last_good.is_none() {
                    last_good = cfg.failover.priority_chain.first().cloned();
                }

                // Защитный отбор кандидатов БС:
                // Формируем пул кандидатов (приоритет bs_priority_chain, затем target_bs_provider/whitelist)
                let mut bs_candidate_ids: Vec<String> = Vec::new();
                let mut seen_cand: std::collections::HashSet<String> = std::collections::HashSet::new();

                for bs_id in &cfg.failover.bs_priority_chain {
                    if !vkturn_ready && bs_id == crate::vkturn::PROXY_NAME {
                        continue;
                    }
                    if !is_quarantined(bs_id).await && seen_cand.insert(bs_id.clone()) {
                        bs_candidate_ids.push(bs_id.clone());
                    }
                }

                for s in servers {
                    if bs_candidate_ids.len() >= 12 {
                        break;
                    }
                    if s.is_pool || s.id == "Fastest" || s.id == "Fallback" || s.id == "DIRECT" || s.id == "REJECT" || s.id == "GLOBAL" || s.id.starts_with("AUTO-") {
                        continue;
                    }
                    if !vkturn_ready
                        && (s.id == crate::vkturn::PROXY_NAME
                            || s.provider.as_deref() == Some(crate::vkturn::PROVIDER_ID))
                    {
                        continue;
                    }
                    if !is_whitelist_server_full(&s.name, &s.id, s.provider.as_deref(), cfg) {
                        continue;
                    }
                    if !det_cfg.target_bs_provider.is_empty() {
                        if let Some(ref p) = s.provider {
                            if !p.eq_ignore_ascii_case(&det_cfg.target_bs_provider) {
                                continue;
                            }
                        } else {
                            continue;
                        }
                    }
                    if is_quarantined(&s.id).await {
                        continue;
                    }
                    if seen_cand.insert(s.id.clone()) {
                        bs_candidate_ids.push(s.id.clone());
                    }
                }

                // Параллельный L7-пинг до 12 кандидатов БС с семафором 12 и таймаутом 1500мс
                let mut bs_target_id: Option<String> = None;
                let mut bs_target_name: Option<String> = None;

                if !bs_candidate_ids.is_empty() {
                    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(12));
                    let mut handles = Vec::new();
                    let shared_cfg = std::sync::Arc::new(cfg.clone());

                    for cand_id in bs_candidate_ids {
                        let http = state.http.clone();
                        let cand_cfg = shared_cfg.clone();
                        let sem_clone = sem.clone();
                        handles.push(tokio::spawn(async move {
                            let _permit = sem_clone.acquire().await.ok();
                            let ping = mihomo::ping_server_url(&http, &cand_cfg, &cand_id, 1500, None).await;
                            (cand_id, ping)
                        }));
                    }

                    let mut responsive: Vec<(String, i64)> = Vec::new();
                    for h in handles {
                        if let Ok((id, ping)) = h.await {
                            if ping > 0 {
                                responsive.push((id, ping));
                            }
                        }
                    }

                    // Выбираем узел с наименьшим пингом
                    responsive.sort_by_key(|(_, ping)| *ping);
                    if let Some((best_id, _)) = responsive.first() {
                        if let Some(s) = servers.iter().find(|srv| &srv.id == best_id) {
                            bs_target_id = Some(s.id.clone());
                            bs_target_name = Some(s.name.clone());
                        }
                    }
                }

                // Fallback: если все кандидаты не ответили по пингу, берем первого не находящегося в карантине из цепочки
                if bs_target_id.is_none() {
                    for bs_id in &cfg.failover.bs_priority_chain {
                        if !vkturn_ready && bs_id == crate::vkturn::PROXY_NAME {
                            continue;
                        }
                        if !is_quarantined(bs_id).await {
                            if let Some(s) = servers.iter().find(|srv| &srv.id == bs_id) {
                                bs_target_id = Some(s.id.clone());
                                bs_target_name = Some(s.name.clone());
                                break;
                            }
                        }
                    }
                }

                let event_msg = if let (Some(target_id), Some(target_name)) = (bs_target_id, bs_target_name) {
                    let switch_res = mihomo::switch_server(&state.http, cfg, &target_id).await;
                    match switch_res {
                        Ok(_) => {
                            record_switch(&target_id).await;
                            let s_prov = servers.iter().find(|s| s.id == target_id).and_then(|s| s.provider.as_deref());
                            handle_whitelist_transition(state, &target_id, &target_name, s_prov).await;

                            // Обновляем конфиг с last_good_regular_vps и bs_priority_chain
                            {
                                let _cfg_guard = state.config_lock.lock().await;
                                let mut mut_cfg = (**state.config.read().await).clone();
                                mut_cfg.failover.last_good_regular_vps = last_good.clone();
                                if let Some(pos) = mut_cfg.failover.bs_priority_chain.iter().position(|s| s == &target_id) {
                                    mut_cfg.failover.bs_priority_chain.remove(pos);
                                }
                                mut_cfg.failover.bs_priority_chain.insert(0, target_id.clone());
                                if let Err(e) = crate::config::save(&state.config_path, &mut_cfg).await {
                                    crate::log_e!("[DETECTOR] Ошибка сохранения конфига: {e}");
                                } else {
                                    *state.config.write().await = std::sync::Arc::new(mut_cfg);
                                }
                            }

                            format!(
                                "🚨 Детектор Белых Списков: обнаружена блокировка ТСПУ ({}% оффлайн: {}/{} узлов). Аварийный переход на сервер [БС] '{}'",
                                offline_pct, offline, sample_total, target_name
                            )
                        }
                        Err(e) => {
                            format!(
                                "🚨 Детектор Белых Списков: обнаружена блокировка ({}% оффлайн), но ошибка переключения на '{}': {}",
                                offline_pct, target_name, e
                            )
                        }
                    }
                } else {
                    format!(
                        "🚨 Детектор Белых Списков: обнаружена блокировка ТСПУ ({}% оффлайн: {}/{} узлов), но подходящий сервер [БС] не найден",
                        offline_pct, offline, sample_total
                    )
                };

                state.failover_log.push(&event_msg, true).await;
                crate::notifications::notify_failover(
                    &state.http,
                    Some(&cfg.mihomo_proxy_url()),
                    &cfg.notifications,
                    "whitelist_detected",
                    "Детектор Белых Списков: Активация [БС]",
                    &event_msg,
                ).await;

                *DETECTOR_LAST_STATUS.lock().await = Some(WhitelistDetectorStatus {
                    state: WhitelistDetectorState::WhitelistDetected,
                    monitored_provider: det_cfg.monitored_provider.clone(),
                    total_nodes: sample_total,
                    monitored_total_nodes: total_candidates,
                    offline_nodes: offline,
                    online_nodes: online,
                    offline_percent: offline_pct,
                    last_checked: Some(now_str),
                    last_event: Some(event_msg.clone()),
                    warning: detector_warning.clone(),
                });

                return Ok(Some(event_msg));
            } else {
                *DETECTOR_LAST_STATUS.lock().await = Some(WhitelistDetectorStatus {
                    state: WhitelistDetectorState::Normal,
                    monitored_provider: det_cfg.monitored_provider.clone(),
                    total_nodes: sample_total,
                    monitored_total_nodes: total_candidates,
                    offline_nodes: offline,
                    online_nodes: online,
                    offline_percent: offline_pct,
                    last_checked: Some(now_str),
                    last_event: None,
                    warning: detector_warning.clone(),
                });
            }
        }
        WhitelistDetectorState::WhitelistDetected => {
            if is_exit_hit {
                // Блокировка снята! Порог выхода достигнут
                set_detector_state(WhitelistDetectorState::Normal).await;

                let mut restored_msg: Option<String> = None;
                if det_cfg.auto_restore_vps {
                    // Защитный отбор кандидата для возврата на обычный VPS:
                    // 1) Не должен быть сервером белых списков (!is_whitelist_server_full).
                    // 2) Приоритеты: last_good_regular_vps -> priority_chain -> любой зарубежный узел.
                    // 3) Выбираем строго из тех, кто ответил alive в недавнем замере (pings) или подтверждён живым быстрым канареечным тестом.
                    let mut candidate_ids: Vec<String> = Vec::new();
                    let mut seen_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

                    if let Some(ref lg) = cfg.failover.last_good_regular_vps {
                        if seen_ids.insert(lg.clone()) {
                            candidate_ids.push(lg.clone());
                        }
                    }
                    for pc in &cfg.failover.priority_chain {
                        if seen_ids.insert(pc.clone()) {
                            candidate_ids.push(pc.clone());
                        }
                    }
                    for s in servers {
                        if candidate_ids.len() >= 16 {
                            break;
                        }
                        if seen_ids.insert(s.id.clone()) {
                            candidate_ids.push(s.id.clone());
                        }
                    }

                    let mut confirmed_target_id: Option<String> = None;
                    let mut confirmed_target_name: Option<String> = None;

                    for cand_id in candidate_ids {
                        let cand_server = match servers.iter().find(|s| s.id == cand_id) {
                            Some(s) => s,
                            None => continue,
                        };

                        if cand_server.is_pool || cand_server.id == "Fastest" || cand_server.id == "Fallback" || cand_server.id == "DIRECT" || cand_server.id == "REJECT" || cand_server.id == "GLOBAL" || cand_server.id.starts_with("AUTO-") {
                            continue;
                        }

                        // Кандидат НЕ должен быть сервером белых списков
                        if is_whitelist_server_full(&cand_server.name, &cand_server.id, cand_server.provider.as_deref(), cfg) {
                            continue;
                        }

                        // Отсев российских нод
                        if det_cfg.exclude_ru_nodes && is_ru_node(&cand_server.name, &cand_server.id) {
                            continue;
                        }

                        // Не должен быть в карантине
                        if is_quarantined(&cand_server.id).await {
                            continue;
                        }

                        // Проверяем доступность: либо подтверждён в pings, либо подтверждаем канареечным L7-тестом
                        let is_alive = if let Some(&p) = pings.get(&cand_server.id) {
                            p > 0 && p <= (det_cfg.max_acceptable_ping_ms as i64)
                        } else {
                            let ms = mihomo::ping_server_url(&state.http, cfg, &cand_server.id, 1500, None).await;
                            ms > 0 && ms <= (det_cfg.max_acceptable_ping_ms as i64)
                        };

                        if is_alive {
                            confirmed_target_id = Some(cand_server.id.clone());
                            confirmed_target_name = Some(cand_server.name.clone());
                            break;
                        }
                    }

                    if let (Some(vps_id), Some(vps_name)) = (confirmed_target_id, confirmed_target_name) {
                        let target_server = servers.iter().find(|s| s.id == vps_id);

                        // Мягкое переключение со сбросом сокетов при переходе с БС на обычный VPS
                        let switch_res = mihomo::switch_server(&state.http, cfg, &vps_id).await;
                        match switch_res {
                            Ok(_) => {
                                record_switch(&vps_id).await;
                                let s_prov = target_server.and_then(|s| s.provider.as_deref());
                                handle_whitelist_transition(state, &vps_id, &vps_name, s_prov).await;

                                let msg = format!(
                                    "🟢 Детектор Белых Списков: блокировка ТСПУ снята (доступно {}/{} узлов, оффлайн {}%). Мягкий возврат на обычный VPS '{}'",
                                    online, sample_total, offline_pct, vps_name
                                );
                                state.failover_log.push(&msg, true).await;
                                crate::notifications::notify_failover(
                                    &state.http,
                                    Some(&cfg.mihomo_proxy_url()),
                                    &cfg.notifications,
                                    "whitelist_restored",
                                    "Детектор Белых Списков: Возврат на VPS",
                                    &msg,
                                ).await;
                                restored_msg = Some(msg);
                            }
                            Err(e) => {
                                let err_msg = format!(
                                    "🟢 Детектор Белых Списков: блокировка снята, но сбой возврата на '{}': {}",
                                    vps_name, e
                                );
                                state.failover_log.push(&err_msg, false).await;
                                restored_msg = Some(err_msg);
                            }
                        }
                    }
                }

                let final_msg = restored_msg.unwrap_or_else(|| {
                    format!(
                        "🟢 Детектор Белых Списков: блокировка снята (доступно {}/{} узлов, оффлайн {}%)",
                        online, sample_total, offline_pct
                    )
                });

                *DETECTOR_LAST_STATUS.lock().await = Some(WhitelistDetectorStatus {
                    state: WhitelistDetectorState::Normal,
                    monitored_provider: det_cfg.monitored_provider.clone(),
                    total_nodes: sample_total,
                    monitored_total_nodes: total_candidates,
                    offline_nodes: offline,
                    online_nodes: online,
                    offline_percent: offline_pct,
                    last_checked: Some(now_str),
                    last_event: Some(final_msg.clone()),
                    warning: detector_warning.clone(),
                });

                return Ok(Some(final_msg));
            } else {
                *DETECTOR_LAST_STATUS.lock().await = Some(WhitelistDetectorStatus {
                    state: WhitelistDetectorState::WhitelistDetected,
                    monitored_provider: det_cfg.monitored_provider.clone(),
                    total_nodes: sample_total,
                    monitored_total_nodes: total_candidates,
                    offline_nodes: offline,
                    online_nodes: online,
                    offline_percent: offline_pct,
                    last_checked: Some(now_str),
                    last_event: None,
                    warning: detector_warning,
                });
            }
        }
    }

    Ok(None)
}

/// Ручной или таймерный запуск проверки детектора белых списков
pub async fn run_whitelist_detector_check(state: &AppState, reason: &str) -> Result<String, String> {
    let cfg = state.config.read().await.clone();
    let speedtest_res = state.speedtest_lock.try_lock();
    let check_res = crate::failover::CHECK_IN_PROGRESS.try_lock();
    let flags = crate::failover::ScanGuardFlags {
        failover_enabled: cfg.failover.enabled || reason == "manual",
        service_stopped: crate::watchdog::is_service_stopped(),
        speedtest_running: speedtest_res.is_err(),
        check_in_progress: check_res.is_err(),
    };
    if let Err(skip) = crate::failover::scan_allowed(flags) {
        return Ok(skip.message("Детектор БС"));
    }
    let _speedtest_guard = speedtest_res.expect("speedtest_res is Ok");
    let _check_guard = check_res.expect("check_res is Ok");

    let servers = mihomo::get_servers(&state.http, &cfg, &cfg.failover.priority_chain).await?;
    match evaluate_and_apply_whitelist_detector(state, &cfg, &servers, reason).await? {
        Some(msg) => Ok(msg),
        None => {
            let status = get_whitelist_detector_status().await;
            Ok(format!(
                "Детектор БС ({reason}): статус {:?}, оффлайн {}% ({}/{} узлов). Переключение не требуется.",
                status.state, status.offline_percent, status.offline_nodes, status.total_nodes
            ))
        }
    }
}

pub async fn current_live_status(
    active_id: &str,
    active_name: &str,
    active_ping: i64,
    cfg: &crate::config::AppConfig,
) -> String {
    if !cfg.failover.enabled {
        return "Failover выключен".to_string();
    }
    if let Some(canary) = canary_status().await {
        return format!(
            "Тестирование кандидата '{}' (Canary Soak: {}/{} с, {} КБ загружено)",
            canary.target_name,
            canary.elapsed_secs,
            canary.duration_secs,
            canary.total_bytes / 1024
        );
    }
    let act_label = if !active_name.is_empty() { active_name } else if !active_id.is_empty() { active_id } else { "не выбран" };
    let ping_str = if active_ping > 0 { format!("{active_ping} мс") } else { "—".to_string() };

    let det_state = get_detector_state().await;
    if det_state == WhitelistDetectorState::WhitelistDetected {
        return format!("🛡️ Режим белых списков активен: '{act_label}' ({ping_str}, мониторинг снятия блокировки)");
    }

    if !cfg.failover.priority_chain.is_empty() {
        if let Some(top_id) = cfg.failover.priority_chain.first() {
            if top_id == active_id || top_id == act_label {
                return format!("Штатно: активен приоритетный узел '{act_label}' ({ping_str})");
            } else {
                return format!("Удержание узла '{act_label}' ({ping_str}, поиск доступных в приоритете)");
            }
        }
    }
    format!("Активен '{act_label}' ({ping_str})")
}
