use axum::extract::State;
use axum::response::{Json, Response};
use serde::Deserialize;
use serde_json::json;

use super::{api_err, api_ok, is_secret_mask};
use crate::AppState;

// =====================================================================
// VK-туннель (ядро csqtt)
// =====================================================================

/// Ссылка для показа в панели без полного хеша звонка (по нему любой может зайти в звонок).
fn mask_call_link(link: &str) -> String {
    match crate::vkturn::parse_call_link(link) {
        Ok(h) => format!("https://vk.com/call/join/{}…", h.chars().take(4).collect::<String>()),
        Err(_) => "—".into(),
    }
}

/// GET /api/vkturn/status
pub async fn get_vkturn_status(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    let rt = state.vkturn.rt.read().await;
    let installed = crate::vkturn::VkTurnManager::core_installed();
    let v = &cfg.vkturn;

    let status_phase = if !installed {
        crate::vkturn::VkTurnPhase::NotInstalled
    } else if !v.enabled {
        crate::vkturn::VkTurnPhase::Disabled
    } else {
        rt.phase
    };

    api_ok(json!({
        "installed": installed,
        "core_version": crate::vkturn::CORE_TAG,
        "phase": status_phase,
        "pid": rt.pid,
        "connected_streams": rt.connected_streams,
        "target_streams": crate::vkturn::effective_workers(v),
        "bytes_up": rt.bytes_up,
        "bytes_down": rt.bytes_down,
        "links_count": v.links.len(),
        "links": v.links.iter().map(|l| mask_call_link(l)).collect::<Vec<_>>(),
        "max_links": crate::vkturn::MAX_LINKS,
        "restarts": rt.restarts,
        "last_error": rt.last_error,
        "error_source": rt.error_source,
        "error_reason": rt.error_reason,
        "enabled": v.enabled,
        "peer": v.peer,
        "has_password": !v.password.is_empty(),
        "tunnel_ready": installed && v.enabled && rt.tunnel_ready(),
        "tunnel_ip": rt.tunnel_ip.map(|ip| ip.to_string()),
        "tunnel_dns": rt.tunnel_dns,
        "interface": crate::vkturn::TUN_IFACE,
        "default_port": crate::vkturn::DEFAULT_SERVER_PORT,
        "install": state.vkturn.install_state(),
        "auth_mode": v.auth_mode,
        "has_token": v.vk_token.as_ref().map(|t| !t.trim().is_empty()).unwrap_or(false),
        "has_profile_token": v.profile_token.as_ref().map(|t| !t.trim().is_empty()).unwrap_or(false),
        "vk_token": v.vk_token.as_ref().map(|t| crate::vkturn::mask_vk_token(t)),
        "vk_user_id": v.vk_user_id,
        "vk_user_name": v.vk_user_name,
        "vk_token_verified": v.vk_token_verified,
    }))
}

/// GET /api/vkturn/logs
pub async fn get_vkturn_logs(State(state): State<AppState>) -> Response {
    let rt = state.vkturn.rt.read().await;
    let lines: Vec<String> = rt.log_tail.iter().cloned().collect();
    api_ok(json!({ "lines": lines }))
}

#[derive(Deserialize)]
pub struct DecryptCsqttProfileReq {
    pub link: String,
}

/// Расшифровка выполняется локально; ссылка и её содержимое не записываются в журнал.
pub async fn decrypt_csqtt_profile(Json(body): Json<DecryptCsqttProfileReq>) -> Response {
    match crate::vkturn::decrypt_csqtt_link(&body.link) {
        Some(link) => api_ok(json!({ "link": link })),
        None => api_err("Не удалось прочитать зашифрованный профиль CSQTTR"),
    }
}

#[derive(Deserialize, Default)]
pub struct SaveVkTurnConfigReq {
    pub enabled: Option<bool>,
    /// Адрес сервера csqtt: "host" или "host:port" (PEER-порт, по умолчанию 46000)
    pub peer: Option<String>,
    /// Пароль подключения; пустая строка или "******" — оставить прежний
    pub password: Option<String>,
    /// Ссылки на звонки ВК по приоритету (полная замена списка)
    pub links: Option<Vec<String>>,
    /// Новая основная ссылка: встаёт первой, остальные (запасные) сохраняются без дублей
    pub primary_link: Option<String>,
    /// Добавить запасную ссылку в конец списка (без дублей)
    pub append_link: Option<String>,
    /// Удалить ссылку по индексу (полезно для удаления битых/скрытых ссылок без пересылки всего списка)
    pub remove_link_index: Option<usize>,
    pub streams: Option<u8>,
    pub auth_mode: Option<String>,
    pub vk_token: Option<String>,
    pub profile_token: Option<String>,
    pub vk_user_id: Option<u64>,
    pub vk_user_name: Option<String>,
}

/// POST /api/vkturn/config
///
/// Блокировки `config_lock`/`routing_lock` захватывает только `ConfigTx::begin` (внутри commit_vkturn).
/// Захват их здесь вручную приводил к вечной взаимной блокировке (мьютексы tokio не реентерабельны).
/// Адрес туннеля сюда не передаётся: его выдаёт сервер csqtt при подключении.
pub async fn save_vkturn_config(
    State(state): State<AppState>,
    Json(body): Json<SaveVkTurnConfigReq>,
) -> Response {
    // Держим блокировку до конца перезапуска: иначе поздно завершившийся запрос мог бы
    // остановить более новый процесс и запустить клиент с уже устаревшими настройками.
    let _update_guard = state.vkturn.config_update_lock.lock().await;
    let mut new_cfg = state.config.read().await.vkturn.clone();

    if let Some(en) = body.enabled {
        new_cfg.enabled = en;
    }
    if let Some(ref p) = body.peer {
        let p_trim = p.trim();
        if let Some(profile) = crate::vkturn::parse_csqtt_profile(p_trim) {
            new_cfg.peer = profile.peer;
            new_cfg.password = profile.password;
            new_cfg.links = profile.links.iter().map(|h| format!("https://vk.com/call/join/{h}")).collect();
            new_cfg.auth_mode = if profile.token.is_some() { "profile_token" } else if !new_cfg.links.is_empty() { "manual" } else { "oauth" }.to_string();
            new_cfg.profile_token = profile.token;
            new_cfg.streams = profile.power.unwrap_or(0);
        } else {
            match crate::vkturn::normalize_peer(p_trim) {
                Ok(n) => new_cfg.peer = n,
                Err(e) => return api_err(format!("Некорректный адрес сервера VPS: {e}")),
            }
        }
    }
    if let Some(ref pw) = body.password {
        let pw = pw.trim();
        if !pw.is_empty() && pw != "******" {
            new_cfg.password = pw.to_string();
        }
    }
    if let Some(links) = body.links {
        let mut out: Vec<String> = Vec::new();
        for (idx, l) in links.iter().map(|s| s.trim()).filter(|s| !s.is_empty()).enumerate() {
            match crate::vkturn::normalize_call_link(l) {
                Ok(n) => {
                    if !out.contains(&n) {
                        out.push(n)
                    }
                }
                Err(e) => return api_err(format!("Ссылка №{} некорректна: {e}", idx + 1)),
            }
        }
        new_cfg.links = out;
    }
    if let Some(ref primary) = body.primary_link {
        match crate::vkturn::with_primary_link(&new_cfg.links, primary) {
            Ok(l) => new_cfg.links = l,
            Err(e) => return api_err(e),
        }
    }
    if let Some(idx) = body.remove_link_index {
        if idx >= new_cfg.links.len() {
            return api_err("Указанная ссылка на звонок не найдена");
        }
        new_cfg.links.remove(idx);
    }
    if let Some(ref extra) = body.append_link {
        let extra = extra.trim();
        if !extra.is_empty() {
            match crate::vkturn::normalize_call_link(extra) {
                Ok(n) => {
                    if !new_cfg.links.contains(&n) {
                        new_cfg.links.push(n);
                    }
                }
                Err(e) => return api_err(format!("Ссылка некорректна: {e}")),
            }
        }
    }
    if let Some(s) = body.streams {
        if s != 0 && !(3..=100).contains(&s) {
            return api_err("Число потоков должно находиться в диапазоне 3–100 (или 0 для автовыбора)");
        }
        new_cfg.streams = s;
    }
    if let Some(am) = body.auth_mode {
        new_cfg.auth_mode = am;
    }
    if let Some(ref tok) = body.vk_token {
        let t = tok.trim();
        if t.is_empty() {
            new_cfg.vk_token = None;
            new_cfg.vk_token_verified = false;
        } else if t != "******" && !t.contains("***") && t != "[REDACTED]" {
            new_cfg.vk_token = Some(t.to_string());
            new_cfg.vk_token_verified = false;
        }
    }
    if let Some(ref tok) = body.profile_token {
        let t = tok.trim();
        if t.is_empty() {
            new_cfg.profile_token = None;
        } else if t.len() > 4096 || t.chars().any(char::is_whitespace) || is_secret_mask(t, false) {
            return api_err("Некорректный токен профиля");
        } else {
            new_cfg.profile_token = Some(t.to_string());
        }
    }
    if let Some(uid) = body.vk_user_id {
        new_cfg.vk_user_id = if uid == 0 { None } else { Some(uid) };
    }
    if let Some(ref name) = body.vk_user_name {
        new_cfg.vk_user_name = Some(name.trim().to_string());
    }
    if new_cfg.links.len() > crate::vkturn::MAX_LINKS {
        return api_err(format!("Можно указать не больше {} ссылок на звонки", crate::vkturn::MAX_LINKS));
    }
    if !new_cfg.password.is_empty() {
        if let Err(e) = crate::vkturn::validate_password(&new_cfg.password) {
            return api_err(e);
        }
    }
    if new_cfg.enabled {
        if new_cfg.peer.is_empty() {
            return api_err("Укажите адрес сервера VPS (IP или домен)");
        }
        if let Err(e) = crate::vkturn::validate_password(&new_cfg.password) {
            return api_err(e);
        }
        let has_valid_token = crate::vkturn::selected_vk_token(&new_cfg).is_some();
        if !has_valid_token && crate::vkturn::call_hashes(&new_cfg).is_empty() {
            return api_err("Укажите хотя бы одну ссылку на звонок ВКонтакте или выполните вход через VK ID");
        }
    }

    let cfg_for_tx = new_cfg.clone();
    let saved = match crate::vkturn::commit_vkturn(&state, move |v| {
        *v = cfg_for_tx;
        Ok(())
    })
    .await
    {
        Ok(c) => c,
        Err(e) => return api_err(e),
    };

    let saved_enabled = saved.enabled;
    state.vkturn.restart(saved, state.clone()).await;
    if !saved_enabled {
        crate::vkturn::evacuate_vkturn_selectors(&state).await;
    }

    api_ok(json!({
        "success": true,
        "message": "Конфигурация VK-туннеля успешно сохранена"
    }))
}

#[derive(Deserialize)]
pub struct VkTurnActionReq {
    pub action: String,
}

/// POST /api/vkturn/action
pub async fn vkturn_action(
    State(state): State<AppState>,
    Json(body): Json<VkTurnActionReq>,
) -> Response {
    let act = body.action.trim();
    match act {
        "enable" | "disable" => {
            let req = SaveVkTurnConfigReq {
                enabled: Some(act == "enable"),
                ..Default::default()
            };
            save_vkturn_config(State(state), Json(req)).await
        }
        "restart" => {
            let cfg = state.config.read().await.vkturn.clone();
            state.vkturn.restart(cfg, state.clone()).await;
            api_ok(json!({
                "success": true,
                "message": "VK-туннель перезапущен"
            }))
        }
        "reset" => {
            let _update_guard = state.vkturn.config_update_lock.lock().await;
            let saved = match crate::vkturn::commit_vkturn(&state, |v| {
                v.enabled = false;
                v.peer = String::new();
                v.password = String::new();
                v.links = Vec::new();
                v.profile_token = None;
                v.auth_mode = "oauth".to_string();
                v.streams = 0;
                Ok(())
            })
            .await
            {
                Ok(c) => c,
                Err(e) => return api_err(e),
            };

            state.vkturn.stop().await;
            *state.vkturn.rt.write().await = crate::vkturn::VkTurnRuntime::default();
            crate::vkturn::evacuate_vkturn_selectors(&state).await;

            let mut masked_cfg = saved.clone();
            if let Some(ref t) = masked_cfg.vk_token {
                masked_cfg.vk_token = Some(crate::vkturn::mask_vk_token(t));
            }
            api_ok(json!({
                "success": true,
                "message": "Настройки сервера VK-туннеля сброшены",
                "config": masked_cfg
            }))
        }
        "install_core" => {
            // Скачивание ядра (~11 МБ) на медленном канале идёт минутами — дольше таймаута запроса
            // веб-панели, поэтому установка идёт в фоне, а ход виден в статусе (поле install).
            if state.vkturn.begin_install() {
                let st = state.clone();
                tokio::spawn(async move {
                    let result = crate::vkturn::install_core(&st).await;
                    match &result {
                        Ok(msg) => crate::log_i!("[VK-туннель] {msg}"),
                        Err(e) => crate::log_w!("[VK-туннель] Установка ядра не удалась: {e}"),
                    }
                    if result.is_ok() {
                        // Если туннель уже включён и ждал ядро — запускаем его сразу.
                        let cfg = st.config.read().await.vkturn.clone();
                        if cfg.enabled {
                            st.vkturn.restart(cfg, st.clone()).await;
                        }
                    }
                    st.vkturn.finish_install(&result);
                });
            }
            api_ok(json!({
                "success": true,
                "started": true,
                "message": "Скачиваю компонент VK-туннеля"
            }))
        }
        "test" => {
            let cfg = state.config.read().await.clone();
            if !state.vkturn.rt.read().await.tunnel_ready() {
                return api_err("Туннель ещё не подключился к серверу csqtt — проверка будет доступна после подключения");
            }
            let delay = crate::mihomo::ping_server_url_expected_range(
                &state.http,
                &cfg,
                crate::vkturn::PROXY_NAME,
                5000,
                Some(cfg.health_check_url()),
                None,
            )
            .await;
            if delay > 0 {
                api_ok(json!({
                    "success": true,
                    "ok": true,
                    "delay_ms": delay,
                    "message": format!("Задержка VK-туннеля: {delay} мс")
                }))
            } else {
                api_err("Узел VK-туннеля недоступен или не отвечает на проверку")
            }
        }
        _ => api_err(format!("Неизвестное действие: {act}")),
    }
}

#[derive(Deserialize, Default)]
pub struct VkTurnAuthSaveReq {
    pub token: Option<String>,
    pub access_token: Option<String>,
    pub user_id: Option<u64>,
    pub url: Option<String>,
    pub auth_mode: Option<String>,
}

/// POST /api/vkturn/auth/save — сохранение токена VK и информации о пользователе
pub async fn save_vkturn_auth(
    State(state): State<AppState>,
    Json(body): Json<VkTurnAuthSaveReq>,
) -> Response {
    let _update_guard = state.vkturn.config_update_lock.lock().await;

    let input_raw = body
        .token
        .as_deref()
        .or(body.access_token.as_deref())
        .or(body.url.as_deref())
        .unwrap_or("")
        .trim();

    if input_raw.is_empty() {
        return api_err("Токен или ссылка авторизации не переданы");
    }

    let (parsed_token, parsed_uid) = match crate::vkturn::parse_vk_redirect_hash(input_raw) {
        Ok((t, u)) => (t, u),
        Err(e) => return api_err(format!("Ошибка разбора данных авторизации: {e}")),
    };

    let target_uid = body.user_id.or(parsed_uid);

    // Запрос профиля пользователя через VK API
    let (real_uid, user_name, verified) = match crate::vkturn::fetch_vk_user_profile(&state.http, &parsed_token, target_uid).await {
        Ok((uid, name)) => (uid, name, true),
        Err(crate::vkturn::VkProfileError::Rejected) => return api_err("VK отклонил токен. Проверьте авторизацию аккаунта."),
        Err(crate::vkturn::VkProfileError::Unavailable) => {
            if state.config.read().await.vkturn.vk_token.as_deref().is_some_and(|t| !t.trim().is_empty()) {
                return api_err("Не удалось проверить новый токен VK. Текущий аккаунт сохранён; попробуйте позже.");
            }
            let uid = target_uid.unwrap_or(0);
            (uid, if uid == 0 { "Профиль VK не проверен".to_string() } else { format!("Пользователь VK {uid}") }, false)
        }
    };

    let auth_mode = body.auth_mode.unwrap_or_else(|| "oauth".to_string());
    let token_for_save = parsed_token.clone();
    let name_for_save = user_name.clone();

    let saved = match crate::vkturn::commit_vkturn(&state, move |v| {
        if v.auth_mode != "manual" && v.auth_mode != "profile_token" {
            v.auth_mode = auth_mode;
        }
        v.vk_token = Some(token_for_save);
        v.vk_token_verified = verified;
        v.vk_user_id = if real_uid > 0 { Some(real_uid) } else { None };
        v.vk_user_name = Some(name_for_save);
        Ok(())
    })
    .await
    {
        Ok(c) => c,
        Err(e) => return api_err(e),
    };

    if saved.enabled && verified && saved.auth_mode != "manual" && saved.auth_mode != "profile_token" {
        state.vkturn.restart(saved, state.clone()).await;
    }

    api_ok(json!({
        "success": true,
        "user_id": real_uid,
        "user_name": user_name,
        "verified": verified,
        "message": if verified { "Учётная запись VK проверена и подключена" } else { "Токен сохранён, но VK недоступен: проверить аккаунт пока не удалось" }
    }))
}

/// POST /api/vkturn/auth/logout — выход из учетной записи VK и очистка токена
pub async fn logout_vkturn_auth(State(state): State<AppState>) -> Response {
    let _update_guard = state.vkturn.config_update_lock.lock().await;

    let saved = match crate::vkturn::commit_vkturn(&state, |v| {
        v.vk_token = None;
        v.vk_user_id = None;
        v.vk_user_name = None;
        v.vk_token_verified = false;
        if v.auth_mode != "manual" && v.auth_mode != "profile_token" {
            v.auth_mode = "oauth".to_string();
        }
        Ok(())
    })
    .await
    {
        Ok(c) => c,
        Err(e) => return api_err(e),
    };

    if saved.enabled && saved.auth_mode != "manual" && saved.auth_mode != "profile_token" {
        state.vkturn.restart(saved, state.clone()).await;
    }

    api_ok(json!({
        "success": true,
        "message": "Вы успешно вышли из учетной записи VK"
    }))
}
