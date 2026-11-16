use axum::extract::State;
use axum::response::{Json, Response};
use serde::Deserialize;
use serde_json::json;

use crate::api::{api_err, api_ok};
use crate::{config, log_i, mihomo, transaction::ConfigTx, AppState};
#[derive(Deserialize)]
pub struct RenameProviderReq {
    #[serde(alias = "id")]
    pub provider_id: String,
    pub alias: String,
}

/// GET /api/providers — список подписок с именами и метаданными
pub async fn get_providers(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    // Актуализируем активного провайдера для фонового учета дельты
    if let Ok(Some(act_srv)) = mihomo::get_active_server(&state.http, &cfg).await {
        crate::traffic::set_active_provider_id(act_srv.provider);
    }
    match mihomo::get_providers_info(&state.http, &cfg).await {
        Ok(providers) => {
            let list: Vec<serde_json::Value> = providers
                .into_iter()
                .map(|p| {
                    let is_wl = crate::failover::is_whitelist_server_full(&p.name, &p.id, Some(&p.id), &cfg);
                    let live_delta = crate::traffic::get_provider_live_delta(&p.id);

                    // Если есть квота (total_bytes), вычисляем actual used (baseline + live_delta) и accurate remaining
                    // Строго официальный отчёт провайдера (Geodema) без двойного начисления локальной дельты
                    let official_used = p.upload_bytes.unwrap_or(0).saturating_add(p.download_bytes.unwrap_or(0));
                    let remaining_bytes = p.total_bytes.map(|tot| tot.saturating_sub(official_used));

                    let mut val = serde_json::to_value(&p).unwrap_or_default();
                    if let Some(obj) = val.as_object_mut() {
                        obj.insert("is_whitelist".to_string(), serde_json::Value::Bool(is_wl));
                        obj.insert("live_delta_bytes".to_string(), json!(live_delta));
                        obj.insert("actual_used_bytes".to_string(), json!(official_used));
                        obj.insert("remaining_bytes".to_string(), json!(remaining_bytes));
                    }
                    val
                })
                .collect();
            api_ok(json!({
                "providers": list,
                "whitelist_providers": cfg.whitelist_providers,
            }))
        }
        Err(e) => api_err(e),
    }
}

/// POST /api/providers/rename — переименование подписки
pub async fn rename_provider(State(state): State<AppState>, Json(req): Json<RenameProviderReq>) -> Response {
    let pid = req.provider_id.trim().to_string();
    if pid.is_empty() {
        return api_err("ID подписки не может быть пустым");
    }
    let alias = req.alias.trim().to_string();
    let _cfg_guard = state.config_lock.lock().await;
    let mut new_cfg = (**state.config.read().await).clone();
    if alias.is_empty() {
        new_cfg.provider_aliases.remove(&pid);
    } else {
        new_cfg.provider_aliases.insert(pid.clone(), alias.clone());
    }
    if let Err(e) = config::save(&state.config_path, &new_cfg).await {
        return api_err(format!("Ошибка сохранения: {e}"));
    }
    *state.config.write().await = std::sync::Arc::new(new_cfg);
    let label = if alias.is_empty() {
        format!("Сброшено имя подписки '{pid}'")
    } else {
        format!("Подписка '{pid}' переименована в '{alias}'")
    };
    api_ok(json!({ "saved": true, "message": label, "provider_id": pid, "alias": alias }))
}

#[derive(Deserialize)]
pub struct ToggleProviderReq {
    #[serde(alias = "id")]
    pub provider_id: String,
    pub enabled: Option<bool>,
}

/// POST /api/providers/toggle — быстрое включение/отключение подписки
pub async fn toggle_provider(State(state): State<AppState>, Json(req): Json<ToggleProviderReq>) -> Response {
    let pid = req.provider_id.trim().to_string();
    if pid.is_empty() {
        return api_err("ID подписки не может быть пустым");
    }

    let mut tx = match ConfigTx::begin(&state).await {
        Ok(t) => t,
        Err(e) => return api_err(format!("Ошибка захвата транзакции: {e}")),
    };

    let is_currently_disabled = tx.config().disabled_providers.iter().any(|p| p.eq_ignore_ascii_case(&pid));
    let target_enabled = req.enabled.unwrap_or(is_currently_disabled);

    if target_enabled {
        tx.config_mut().disabled_providers.retain(|p| !p.eq_ignore_ascii_case(&pid));
    } else if !is_currently_disabled {
        tx.config_mut().disabled_providers.push(pid.clone());
    }

    let yaml = match tx.read_yaml().await {
        Ok(y) => y,
        Err(e) => return api_err(e),
    };

    let synced_yaml = match crate::routing::sync_disabled_providers_in_yaml(&yaml, &tx.config().disabled_providers) {
        Ok(y) => y,
        Err(e) => return api_err(e),
    };

    let (routed_yaml, _) = match crate::routing::apply_routing(&synced_yaml, tx.config()) {
        Ok(r) => r,
        Err(e) => return api_err(e),
    };

    if let Err(e) = tx.set_yaml(routed_yaml) {
        return api_err(format!("Ошибка валидации YAML: {e}"));
    }

    if let Err(e) = tx.commit_and_reload().await {
        return api_err(format!("Ошибка сохранения и перезагрузки: {e}"));
    }

    let disabled_list = state.config.read().await.disabled_providers.clone();
    let label = if target_enabled {
        format!("Подписка '{pid}' включена")
    } else {
        format!("Подписка '{pid}' отключена")
    };
    api_ok(json!({
        "saved": true,
        "message": label,
        "provider_id": pid,
        "enabled": target_enabled,
        "disabled_providers": disabled_list,
    }))
}

#[derive(Deserialize)]
pub struct UpdateProviderReq {
    #[serde(alias = "id")]
    pub provider_id: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderKind {
    Http,
    MobileFile,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProviderUpdateAction {
    KeepHttp,
    SaveConvertedFile,
    Reject(String),
}

/// Решение о действии при ручном обновлении подписки (баг A-13):
/// обычный HTTP-провайдер никогда не переводится неявно в `type: file`.
pub fn provider_update_action(
    provider_kind: ProviderKind,
    body_kind: SubscriptionBodyKind,
) -> ProviderUpdateAction {
    match (provider_kind, body_kind) {
        (ProviderKind::MobileFile, SubscriptionBodyKind::Base64OrUri) => {
            ProviderUpdateAction::SaveConvertedFile
        }
        (ProviderKind::MobileFile, SubscriptionBodyKind::NativeYaml) => {
            ProviderUpdateAction::Reject(
                "Мобильная подписка вернула YAML-файл вместо списка серверов".to_string(),
            )
        }
        (ProviderKind::Http, SubscriptionBodyKind::NativeYaml)
        | (ProviderKind::Http, SubscriptionBodyKind::Base64OrUri) => {
            ProviderUpdateAction::KeepHttp
        }
        (_, SubscriptionBodyKind::Unknown) => ProviderUpdateAction::Reject(
            "Подписка вернула пустой или непонятный ответ — обновление отклонено".to_string(),
        ),
    }
}

/// POST /api/providers/update — принудительное обновление подписки/подписок
pub async fn update_provider(State(state): State<AppState>, Json(req): Json<UpdateProviderReq>) -> Response {
    let cfg = state.config.read().await.clone();

    let yaml_content = tokio::fs::read_to_string(&cfg.mihomo.config_path).await.unwrap_or_default();
    let provider_hwids = crate::routing::parse_provider_hwids(&yaml_content);

    if let Some(pid) = req.provider_id.filter(|p| !p.trim().is_empty()) {
        let hwid = provider_hwids.get(&pid).map(|s| s.as_str());
        // 1. Мобильные подписки (зарегистрированные пользователем в mobile_provider_urls)
        if let Some(mobile_url) = cfg.mobile_provider_urls.get(&pid) {
            match provider_update_action(ProviderKind::MobileFile, SubscriptionBodyKind::Base64OrUri) {
                ProviderUpdateAction::SaveConvertedFile => {
                    match refresh_mobile_sub(&state, &pid, mobile_url, hwid).await {
                        Ok(count) => {
                            let _ = mihomo::force_update_provider(&state.http, &cfg, &pid).await;
                            return api_ok(json!({ "updated": 1, "message": format!("Мобильная подписка '{pid}' обновлена ({count} серверов)") }));
                        }
                        Err(e) => return api_err(format!("Ошибка обновления мобильной подписки '{pid}': {e}")),
                    }
                }
                ProviderUpdateAction::Reject(reason) => return api_err(reason),
                ProviderUpdateAction::KeepHttp => {}
            }
        }

        // 2. Обычный HTTP-провайдер остаётся type: http и обновляется штатно через Mihomo (баг A-13)
        match mihomo::force_update_provider(&state.http, &cfg, &pid).await {
            Ok(_) => api_ok(json!({ "updated": 1, "message": format!("Подписка '{pid}' обновлена") })),
            Err(e) => api_err(format!("Ошибка обновления подписки '{pid}': {e}")),
        }
    } else {
        let mobile_count = sync_all_mobile_providers(&state).await;
        let count = mihomo::force_update_all_providers(&state.http, &cfg).await;
        api_ok(json!({ "updated": count + mobile_count, "message": format!("Обновлено подписок: {}", count + mobile_count) }))
    }
}

/// Разделение вывода curl -i на HTTP-заголовки (включая мультиблочные заголовки CONNECT-прокси) и тело
pub fn split_curl_response(raw: &str) -> (Option<crate::mihomo::SubscriptionUserinfo>, String) {
    let mut curr = raw;
    let mut userinfo = None;

    // Цикл пропускает все блоки HTTP-заголовков (включая HTTP/1.1 200 Connection established от прокси и редиректы)
    while !curr.is_empty() {
        let trimmed_leading = curr.trim_start_matches(|c| c == '\r' || c == '\n');
        let lower = trimmed_leading.to_ascii_lowercase();
        if !lower.starts_with("http/") {
            break;
        }

        // Ищем конец текущего блока заголовков
        let sep_info = ["\r\n\r\n", "\n\n", "\r\r"]
            .iter()
            .filter_map(|sep| trimmed_leading.find(sep).map(|pos| (pos, sep.len())))
            .min_by_key(|&(pos, _)| pos);

        if let Some((pos, sep_len)) = sep_info {
            let header_block = &trimmed_leading[..pos];
            if userinfo.is_none() {
                userinfo = header_block
                    .lines()
                    .find(|l| l.to_ascii_lowercase().starts_with("subscription-userinfo:"))
                    .and_then(|l| l.split_once(':').map(|(_, v)| v.trim()))
                    .and_then(crate::mihomo::parse_subscription_userinfo_str);
            }
            curr = &trimmed_leading[pos + sep_len..];
        } else {
            break;
        }
    }

    // Если всё ещё остался префикс до структуры тела (например, в случае нестандартных прокси)
    let body_str = if let Some(bracket_pos) = curr.find("\n[") {
        &curr[bracket_pos + 1..]
    } else if let Some(brace_pos) = curr.find("\n{") {
        &curr[brace_pos + 1..]
    } else if let Some(proxies_pos) = curr.find("\nproxies:") {
        &curr[proxies_pos + 1..]
    } else {
        curr
    };

    if userinfo.is_none() {
        userinfo = raw.lines()
            .find(|l| l.to_ascii_lowercase().starts_with("subscription-userinfo:"))
            .and_then(|l| l.split_once(':').map(|(_, v)| v.trim()))
            .and_then(crate::mihomo::parse_subscription_userinfo_str);
    }

    (userinfo, body_str.trim().to_string())
}

fn curl_escape_value(v: &str) -> String {
    v.replace(['\r', '\n'], "")
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

/// Аргументы запуска curl для загрузки мобильной подписки без утечки URL/HWID в argv (`ps`).
pub fn build_curl_args() -> Vec<String> {
    vec![
        "-s".to_string(),
        "-i".to_string(),
        "-f".to_string(),
        "-L".to_string(),
        "--max-redirs".to_string(),
        "5".to_string(),
        "--max-time".to_string(),
        "15".to_string(),
        "-K".to_string(),
        "-".to_string(),
    ]
}

/// Конфиг для `curl -K -` (передаётся через stdin, скрывая URL, User-Agent и x-hwid от `ps`).
pub fn build_curl_stdin_config(url: &str, proxy_url: &str, ua: &str, hwid: Option<&str>) -> String {
    let mut lines = vec![
        format!("url = \"{}\"", curl_escape_value(url)),
        format!("proxy = \"{}\"", curl_escape_value(proxy_url)),
        format!("header = \"User-Agent: {}\"", curl_escape_value(ua)),
    ];
    if let Some(h) = hwid.filter(|s| !s.is_empty()) {
        lines.push(format!("header = \"x-hwid: {}\"", curl_escape_value(h)));
    }
    lines.join("\n")
}

/// Атомарная запись сконвертированного файла мобильного провайдера (tmp + rename).
pub async fn write_mobile_provider_yaml_atomic(
    prov_path: &std::path::Path,
    conv_yaml: &str,
) -> Result<(), String> {
    crate::api::atomic_write_file(prov_path, conv_yaml)
        .await
        .map_err(|e| format!("Запись {}: {e}", prov_path.display()))
}

/// Загрузка мобильной подписки (V2Ray/JSON/Base64) через прокси Mihomo и сохранение в ./providers/{id}.yaml
pub async fn refresh_mobile_sub(state: &AppState, id: &str, url: &str, hwid: Option<&str>) -> Result<usize, String> {
    let ua = "v2rayNG/1.8.12";
    let (proxy_url, resolved_hwid) = {
        let cfg = state.config.read().await;
        let h = hwid
            .filter(|h| !h.is_empty())
            .map(|s| s.to_string())
            .or_else(|| cfg.mobile_provider_hwids.get(id).cloned());
        (cfg.mihomo_proxy_url(), h)
    };
    let proxied = crate::updater::proxied_client(&proxy_url);
    let client_ref = proxied.as_ref().unwrap_or(&state.http);
    let mut req_builder = client_ref.get(url).header("User-Agent", ua).timeout(std::time::Duration::from_secs(12));
    if let Some(ref h) = resolved_hwid {
        req_builder = req_builder.header("x-hwid", h.as_str());
    }
    // 1. Сначала пробуем получить подписку через системный curl роутера (/opt/bin/curl или curl)
    // (curl на роутере идеально согласует TLS/ALPN с прокси Mihomo и сервером QRATOR)
    let curl_bin = if std::path::Path::new("/opt/bin/curl").exists() {
        "/opt/bin/curl"
    } else {
        "curl"
    };
    let mut cmd = tokio::process::Command::new(curl_bin);
    cmd.args(build_curl_args())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let stdin_cfg = build_curl_stdin_config(url, &proxy_url, ua, resolved_hwid.as_deref());
    let curl_output = match cmd.spawn() {
        Ok(mut child) => {
            if let Some(mut stdin) = child.stdin.take() {
                use tokio::io::AsyncWriteExt;
                let _ = stdin.write_all(stdin_cfg.as_bytes()).await;
                let _ = stdin.shutdown().await;
            }
            child.wait_with_output().await
        }
        Err(e) => Err(e),
    };

    let (mut header_userinfo, text) = if let Ok(out) = curl_output {
        if out.status.success() && !out.stdout.is_empty() {
            let full_str = String::from_utf8_lossy(&out.stdout).to_string();
            split_curl_response(&full_str)
        } else {
            (None, String::new())
        }
    } else {
        (None, String::new())
    };

    let text = if text.is_empty() {
        let resp = match req_builder.send().await {
            Ok(r) if r.status().is_success() => r,
            _ => {
                let mut direct_builder = state.http.get(url).header("User-Agent", ua).timeout(std::time::Duration::from_secs(12));
                if let Some(ref h) = resolved_hwid {
                    direct_builder = direct_builder.header("x-hwid", h.as_str());
                }
                direct_builder.send().await.map_err(|e| format!("HTTP {e}"))?
            }
        };
        if !resp.status().is_success() {
            return Err(format!("HTTP статус {}", resp.status()));
        }
        let headers = resp.headers();
        if header_userinfo.is_none() {
            header_userinfo = headers
                .iter()
                .find(|(k, _)| k.as_str().eq_ignore_ascii_case("subscription-userinfo"))
                .and_then(|(_, v)| v.to_str().ok())
                .and_then(crate::mihomo::parse_subscription_userinfo_str);
        }
        let bytes = match resp.bytes().await {
            Ok(b) => b,
            Err(e) => return Err(format!("Read bytes: {e}")),
        };
        String::from_utf8_lossy(&bytes).to_string()
    } else {
        text
    };

    // Если основной запрос (например, с UA v2rayNG) не вернул заголовок Subscription-Userinfo,
    // делаем лёгкий запрос (HEAD или GET с Range: bytes=0-0) с ClashMeta UA для гарантированного получения квоты
    if header_userinfo.is_none() {
        let clash_ua = "ClashMeta/1.19.24";
        let mut meta_builder = client_ref.get(url).header("User-Agent", clash_ua).header("Range", "bytes=0-0").timeout(std::time::Duration::from_secs(6));
        if let Some(ref h) = resolved_hwid {
            meta_builder = meta_builder.header("x-hwid", h.as_str());
        }
        if let Ok(meta_resp) = meta_builder.send().await {
            let u = meta_resp.headers()
                .iter()
                .find(|(k, _)| k.as_str().eq_ignore_ascii_case("subscription-userinfo"))
                .and_then(|(_, v)| v.to_str().ok())
                .and_then(crate::mihomo::parse_subscription_userinfo_str);
            if u.is_some() {
                header_userinfo = u;
            }
        }
    }
    let (mut conv_yaml, count) = crate::subscription_parser::convert_subscription_to_mihomo_yaml(&text)?;
    let prov_dir = std::path::Path::new(&state.config.read().await.mihomo.config_path)
        .parent()
        .map(|p| p.join("providers"))
        .unwrap_or_else(|| std::path::PathBuf::from("/opt/etc/mihomo/providers"));
    let prov_path = prov_dir.join(format!("{id}.yaml"));
    if let Some(parent) = prov_path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }

    // Preservation and injection of Subscription-Userinfo
    let final_userinfo = if let Some(u) = header_userinfo {
        crate::traffic::reset_provider_live_delta(id);
        Some(u)
    } else if let Some(u) = crate::mihomo::extract_subscription_userinfo_from_body(&text) {
        crate::traffic::reset_provider_live_delta(id);
        Some(u)
    } else if let Ok(existing_yaml) = tokio::fs::read_to_string(&prov_path).await {
        crate::mihomo::extract_subscription_userinfo_from_body(&existing_yaml)
    } else {
        None
    };

    if let Some(ref u) = final_userinfo {
        conv_yaml = crate::mihomo::inject_or_replace_userinfo_in_yaml(&conv_yaml, u);
    }

    write_mobile_provider_yaml_atomic(&prov_path, &conv_yaml).await?;
    Ok(count)
}

/// Фоновое автообновление и автомиграция всех мобильных подписок:
/// Проверяет зарегистрированные mobile_provider_urls, а также автоматически мигрирует
/// любые HTTP-провайдеры, отдающие V2Ray/JSON подписки, в локальный type: file.
/// Фоновое обновление мобильных подписок (баг 80): только те, что пользователь сам перевёл в мобильный
/// формат (mobile_provider_urls). Обычные http-провайдеры Mihomo обновляет сам, их тип не меняется.
pub async fn sync_all_mobile_providers(state: &AppState) -> usize {
    let cfg = state.config.read().await.clone();
    let yaml_content = tokio::fs::read_to_string(&cfg.mihomo.config_path).await.unwrap_or_default();
    let provider_hwids = crate::routing::parse_provider_hwids(&yaml_content);

    let mut mobile_count = 0;
    for (pid, mobile_url) in &cfg.mobile_provider_urls {
        let hwid = provider_hwids
            .get(pid)
            .map(|s| s.as_str())
            .or_else(|| cfg.mobile_provider_hwids.get(pid).map(|s| s.as_str()));
        if let Ok(count) = refresh_mobile_sub(state, pid, mobile_url, hwid).await {
            let _ = mihomo::force_update_provider(&state.http, &cfg, pid).await;
            log_i!("[MOBILE-SUB] ✓ Мобильная подписка '{pid}' обновлена ({count} серверов)");
            mobile_count += 1;
        }
    }
    mobile_count
}

#[derive(Deserialize)]
pub struct AddProviderReq {
    pub id: String,
    pub url: String,
    pub name: Option<String>,
    pub hwid: Option<String>,
    pub user_agent: Option<String>,
    pub append_hwid_to_url: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubscriptionBodyKind {
    NativeYaml,
    Base64OrUri,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderPath {
    NativeHttp,
    MobileFile,
}

/// Нейтральный User-Agent по умолчанию для обычных подписок (`clash.meta`/`mihomo`),
/// `v2rayNG` используется только при явном выборе пользователя (баг 81).
pub fn default_provider_user_agent(explicit_ua: Option<&str>) -> &str {
    explicit_ua
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("clash.meta/mihomo")
}

/// Определение формата тела подписки (нативный Clash/Mihomo YAML vs Base64/URI-список).
pub fn detect_subscription_body_kind(body: &str) -> SubscriptionBodyKind {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return SubscriptionBodyKind::Unknown;
    }
    if crate::subscription_parser::is_v2ray_or_base64_subscription(trimmed) {
        return SubscriptionBodyKind::Base64OrUri;
    }
    if trimmed.lines().any(|l| {
        let lt = l.trim_start();
        lt.starts_with("proxies:") || lt.starts_with("proxy-groups:")
    }) {
        return SubscriptionBodyKind::NativeYaml;
    }
    SubscriptionBodyKind::Unknown
}

/// Выбор пути добавления провайдера по явному мобильному UA и типу тела подписки (баг 81).
pub fn choose_provider_path(explicit_mobile_ua: bool, body_kind: SubscriptionBodyKind) -> ProviderPath {
    match body_kind {
        SubscriptionBodyKind::NativeYaml => ProviderPath::NativeHttp,
        SubscriptionBodyKind::Base64OrUri => ProviderPath::MobileFile,
        SubscriptionBodyKind::Unknown => {
            if explicit_mobile_ua {
                ProviderPath::MobileFile
            } else {
                ProviderPath::NativeHttp
            }
        }
    }
}

/// POST /api/providers/add — добавление новой подписки в config.yaml
pub async fn add_provider(State(state): State<AppState>, Json(req): Json<AddProviderReq>) -> Response {
    let id = req.id.trim();
    let raw_url = req.url.trim();
    if id.is_empty() || raw_url.is_empty() {
        return api_err("ID и URL подписки не могут быть пустыми");
    }
    if !crate::config::is_valid_provider_id(id) {
        return api_err("Некорректный ID подписки: разрешены только символы [a-zA-Z0-9_-], длина до 64 символов");
    }

    let hwid_trimmed = req.hwid.as_deref().map(str::trim).filter(|h| !h.is_empty());
    let mut final_url = raw_url.to_string();
    if req.append_hwid_to_url.unwrap_or(false) {
        if let Some(h) = hwid_trimmed {
            if !final_url.contains("hwid=") {
                let sep = if final_url.contains('?') { '&' } else { '?' };
                final_url.push(sep);
                final_url.push_str(&format!("hwid={h}"));
            }
        }
    }

    // Проверяем, является ли подписка мобильной V2Ray/Base64: только по явному мобильному UA или по факту формата тела (баг 81)
    let mut mobile_yaml_opt: Option<(String, usize)> = None;
    let is_explicit_mobile_ua = req.user_agent.as_deref().map_or(false, |u| {
        let l = u.to_lowercase();
        l.contains("v2ray") || l.contains("happ") || l.contains("singbox") || l.contains("sing-box")
    });

    if final_url.starts_with("http://") || final_url.starts_with("https://") {
        let ua = default_provider_user_agent(req.user_agent.as_deref());
        let proxy_url = {
            let cfg = state.config.read().await;
            cfg.mihomo_proxy_url()
        };
        let proxied = crate::updater::proxied_client(&proxy_url);
        let client_ref = proxied.as_ref().unwrap_or(&state.http);
        let mut req_builder = client_ref.get(&final_url).header("User-Agent", ua);
        if let Some(h) = hwid_trimmed {
            req_builder = req_builder.header("x-hwid", h);
        }
        let resp = match req_builder.send().await {
            Ok(r) if r.status().is_success() => Ok(r),
            _ => {
                let mut direct_builder = state.http.get(&final_url).header("User-Agent", ua);
                if let Some(h) = hwid_trimmed {
                    direct_builder = direct_builder.header("x-hwid", h);
                }
                direct_builder.send().await
            }
        };
        if let Ok(resp) = resp {
            if resp.status().is_success() {
                let userinfo_hdr = resp
                    .headers()
                    .iter()
                    .find(|(k, _)| k.as_str().eq_ignore_ascii_case("subscription-userinfo"))
                    .and_then(|(_, v)| v.to_str().ok())
                    .and_then(crate::mihomo::parse_subscription_userinfo_str);
                if let Ok(text) = resp.text().await {
                    let body_kind = detect_subscription_body_kind(&text);
                    if choose_provider_path(is_explicit_mobile_ua, body_kind) == ProviderPath::MobileFile
                        && body_kind == SubscriptionBodyKind::Base64OrUri
                    {
                        if let Ok((mut conv_yaml, count)) = crate::subscription_parser::convert_subscription_to_mihomo_yaml(&text) {
                            let parsed_ui = userinfo_hdr.or_else(|| crate::mihomo::extract_subscription_userinfo_from_body(&text));
                            if let Some(ref u) = parsed_ui {
                                conv_yaml = crate::mihomo::inject_or_replace_userinfo_in_yaml(&conv_yaml, u);
                            }
                            mobile_yaml_opt = Some((conv_yaml, count));
                        }
                    }
                }
            }
        }
    }

    let mut tx = match ConfigTx::begin(&state).await {
        Ok(t) => t,
        Err(e) => return api_err(format!("Ошибка захвата транзакции: {e}")),
    };

    let yaml = match tx.read_yaml().await {
        Ok(y) => y,
        Err(e) => return api_err(e),
    };

    let (new_yaml, parsed_mobile_count) = if let Some((conv_yaml, count)) = mobile_yaml_opt {
        let providers_dir = std::path::Path::new(&tx.config().mihomo.config_path)
            .parent()
            .map(|p| p.join("providers"))
            .unwrap_or_else(|| std::path::PathBuf::from("/opt/etc/mihomo/providers"));
        let prov_file = providers_dir.join(format!("{id}.yaml"));
        if let Err(e) = tx.set_extra_file(&prov_file, conv_yaml, true).await {
            return api_err(format!("Ошибка подготовки файла провайдера: {e}"));
        }
        let ny = match crate::routing::add_file_provider_to_yaml(
            &yaml,
            id,
            &format!("./providers/{id}.yaml"),
            Some(tx.config().health_check_url()),
            Some(tx.config().mihomo.health_check_interval),
        ) {
            Ok(y) => y,
            Err(e) => return api_err(e),
        };
        tx.config_mut().mobile_provider_urls.insert(id.to_string(), final_url.clone());
        if let Some(h) = hwid_trimmed {
            tx.config_mut().mobile_provider_hwids.insert(id.to_string(), h.to_string());
        }
        (ny, Some(count))
    } else {
        let ny = match crate::routing::add_provider_to_yaml_full(
            &yaml,
            id,
            &final_url,
            Some(tx.config().health_check_url()),
            Some(tx.config().mihomo.health_check_interval),
            hwid_trimmed,
            req.user_agent.as_deref(),
        ) {
            Ok(y) => y,
            Err(e) => return api_err(e),
        };
        (ny, None)
    };

    if let Err(e) = tx.set_yaml(new_yaml) {
        return api_err(format!("Ошибка валидации YAML: {e}"));
    }

    // Если указано пользовательское имя — сохраняем псевдоним
    if let Some(alias) = req.name.filter(|n| !n.trim().is_empty()) {
        tx.config_mut().provider_aliases.insert(id.to_string(), alias.trim().to_string());
    }

    if let Err(e) = tx.commit_and_reload().await {
        return api_err(format!("Ошибка сохранения и применения подписки: {e}"));
    }

    let cfg = state.config.read().await.clone();
    let _ = mihomo::force_update_provider(&state.http, &cfg, id).await;

    let msg = if let Some(count) = parsed_mobile_count {
        format!("Мобильная подписка '{id}' (V2Ray / Base64) успешно добавлена ({count} серверов с именами как на телефоне)")
    } else {
        format!("Подписка '{id}' успешно добавлена")
    };

    api_ok(json!({ "added": true, "message": msg }))
}

#[derive(Deserialize)]
pub struct DeleteProviderReq {
    #[serde(alias = "provider_id")]
    pub id: String,
}

/// POST /api/providers/delete — удаление подписки из config.yaml
pub async fn delete_provider(State(state): State<AppState>, Json(req): Json<DeleteProviderReq>) -> Response {
    let id = req.id.trim();
    if id.is_empty() {
        return api_err("ID подписки не может быть пустым");
    }
    if !crate::config::is_valid_provider_id(id) {
        return api_err("Некорректный ID подписки: разрешены только символы [a-zA-Z0-9_-], длина до 64 символов");
    }

    let mut tx = match ConfigTx::begin(&state).await {
        Ok(t) => t,
        Err(e) => return api_err(format!("Ошибка захвата транзакции: {e}")),
    };

    let yaml = match tx.read_yaml().await {
        Ok(y) => y,
        Err(e) => return api_err(e),
    };

    let new_yaml = match crate::routing::delete_provider_from_yaml(&yaml, id) {
        Ok(y) => y,
        Err(e) => return api_err(e),
    };

    if let Err(e) = tx.set_yaml(new_yaml) {
        return api_err(format!("Ошибка валидации YAML: {e}"));
    }

    // Удаляем псевдоним и фильтры из AppConfig
    tx.config_mut().provider_aliases.remove(id);
    tx.config_mut().provider_filters.remove(id);
    tx.config_mut().mobile_provider_urls.remove(id);
    tx.config_mut().mobile_provider_hwids.remove(id);

    if let Err(e) = tx.commit_and_reload().await {
        return api_err(format!("Ошибка сохранения config.yaml: {e}"));
    }

    // Удаляем кэш-файлы провайдера с диска (если создавались)
    let prov_cache_yaml = format!("/opt/etc/mihomo/providers/{id}.yaml");
    let prov_cache_json = format!("/opt/etc/mihomo/providers/{id}.json");
    let _ = tokio::fs::remove_file(&prov_cache_yaml).await;
    let _ = tokio::fs::remove_file(&prov_cache_json).await;

    api_ok(json!({ "deleted": true, "message": format!("Подписка '{id}' успешно удалена") }))
}

#[derive(Deserialize)]
pub struct UpdateProviderHwidReq {
    pub id: String,
    pub hwid: Option<String>,
}

/// POST /api/providers/hwid — обновление или удаление HWID подписки в config.yaml
pub async fn update_provider_hwid(
    State(state): State<AppState>,
    Json(req): Json<UpdateProviderHwidReq>,
) -> Response {
    let id = req.id.trim();
    if id.is_empty() {
        return api_err("ID подписки не может быть пустым");
    }

    let mut tx = match ConfigTx::begin(&state).await {
        Ok(t) => t,
        Err(e) => return api_err(format!("Ошибка захвата транзакции: {e}")),
    };

    let yaml = match tx.read_yaml().await {
        Ok(y) => y,
        Err(e) => return api_err(e),
    };

    let new_yaml = match crate::routing::update_provider_hwid_in_yaml(&yaml, id, req.hwid.as_deref()) {
        Ok(y) => y,
        Err(e) => return api_err(e),
    };

    if let Err(e) = tx.set_yaml(new_yaml) {
        return api_err(format!("Ошибка валидации YAML: {e}"));
    }

    if let Err(e) = tx.commit_and_reload().await {
        return api_err(format!("Ошибка сохранения и применения HWID: {e}"));
    }

    let cfg = state.config.read().await.clone();
    let _ = mihomo::force_update_provider(&state.http, &cfg, id).await;

    let msg = match req.hwid.as_deref().map(str::trim).filter(|h| !h.is_empty()) {
        Some(h) => format!("HWID для подписки '{id}' успешно обновлён на '{h}'"),
        None => format!("HWID для подписки '{id}' успешно удалён"),
    };

    api_ok(json!({ "updated": true, "message": msg, "id": id, "hwid": req.hwid }))
}

#[derive(Deserialize)]
pub struct FreeSubConnectReq {
    #[serde(default)]
    pub mode: crate::free_sub::FreeSubMode,
}

/// POST /api/providers/free-sub/connect — запуск подключения бесплатной мульти-подписки в 1 клик.
pub async fn connect_free_sub(
    State(state): State<AppState>,
    Json(req): Json<FreeSubConnectReq>,
) -> Response {
    let cur = crate::free_sub::get_status().await;
    if cur.running {
        return api_ok(json!({
            "started": false,
            "already_running": true,
            "status": cur,
        }));
    }
    let st_clone = state.clone();
    let mode = req.mode;
    tokio::spawn(async move {
        let _ = crate::free_sub::start_one_click_connect(st_clone, mode).await;
    });
    api_ok(json!({
        "started": true,
        "mode": mode,
    }))
}

/// GET /api/providers/free-sub/status — текущий статус и диагностика конвейера бесплатной подписки.
pub async fn get_free_sub_status() -> Response {
    let st = crate::free_sub::get_status().await;
    api_ok(json!({ "status": st }))
}

#[derive(Deserialize)]
pub struct FreeSubModeReq {
    #[serde(default)]
    pub mode: crate::free_sub::FreeSubMode,
}

#[derive(Deserialize)]
pub struct FreeSubSwitchNodeReq {
    #[serde(default)]
    pub mode: crate::free_sub::FreeSubMode,
    #[serde(default)]
    pub direction: Option<String>,
    #[serde(default)]
    pub node_name: Option<String>,
}

#[derive(Deserialize)]
pub struct FreeSubBlacklistReq {
    #[serde(default)]
    pub mode: crate::free_sub::FreeSubMode,
    #[serde(default)]
    pub node_name: Option<String>,
}

#[derive(Deserialize)]
pub struct FreeSubPoolQuery {
    #[serde(default)]
    pub mode: crate::free_sub::FreeSubMode,
}

/// POST /api/providers/free-sub/enable — добавить обе виртуальные карточки (`free_vpn` и `free_wl`) в 1 клик.
pub async fn enable_free_sub(State(state): State<AppState>) -> Response {
    match crate::free_sub::enable_both_free_subs(&state).await {
        Ok(()) => api_ok(json!({ "enabled": true })),
        Err(e) => api_err(e),
    }
}

/// POST /api/providers/free-sub/disconnect — отключить активную бесплатную подписку с возвратом прежнего сервера.
pub async fn disconnect_free_sub(
    State(state): State<AppState>,
    Json(req): Json<FreeSubModeReq>,
) -> Response {
    match crate::free_sub::disconnect_free_sub(&state, req.mode).await {
        Ok(restored) => api_ok(json!({ "disconnected": true, "restored_server": restored })),
        Err(e) => api_err(e),
    }
}

/// POST /api/providers/free-sub/remove — удалить/выключить бесплатную подписку и её карточку.
pub async fn remove_free_sub(
    State(state): State<AppState>,
    Json(req): Json<FreeSubModeReq>,
) -> Response {
    match crate::free_sub::remove_free_sub(&state, req.mode).await {
        Ok(()) => api_ok(json!({ "removed": true, "mode": req.mode })),
        Err(e) => api_err(e),
    }
}

/// POST /api/providers/free-sub/switch-node — переключение узла в пуле (`next` / `prev` / `node_name`).
pub async fn switch_free_sub_node(
    State(state): State<AppState>,
    Json(req): Json<FreeSubSwitchNodeReq>,
) -> Response {
    let target = req
        .node_name
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .or(req.direction.as_deref())
        .unwrap_or("next");
    match crate::free_sub::switch_pool_node(&state, req.mode, target).await {
        Ok(node) => api_ok(json!({ "active_node": node, "mode": req.mode })),
        Err(e) => api_err(e),
    }
}

/// POST /api/providers/free-sub/blacklist-node — заблокировать плохой узел на 24ч и переключиться на следующий.
pub async fn blacklist_free_sub_node(
    State(state): State<AppState>,
    Json(req): Json<FreeSubBlacklistReq>,
) -> Response {
    match crate::free_sub::blacklist_and_next(&state, req.mode, req.node_name.as_deref()).await {
        Ok(node) => api_ok(json!({ "active_node": node, "mode": req.mode })),
        Err(e) => api_err(e),
    }
}

/// POST /api/providers/free-sub/refill — фоновый добор свежих узлов без обрыва текущей сессии.
pub async fn refill_free_sub(
    State(state): State<AppState>,
    Json(req): Json<FreeSubModeReq>,
) -> Response {
    let cur = crate::free_sub::get_status().await;
    if cur.running {
        return api_ok(json!({ "started": false, "already_running": true, "status": cur }));
    }
    let st_clone = state.clone();
    let mode = req.mode;
    tokio::spawn(async move {
        let _ = crate::free_sub::start_background_refill(st_clone, mode).await;
    });
    api_ok(json!({ "started": true, "mode": mode }))
}

/// GET /api/providers/free-sub/pool — список узлов пула, счётчик и история сессий.
pub async fn get_free_sub_pool(
    State(state): State<AppState>,
    axum::extract::Query(q): axum::extract::Query<FreeSubPoolQuery>,
) -> Response {
    let data = crate::free_sub::get_pool_and_history(&state, q.mode).await;
    api_ok(data)
}
