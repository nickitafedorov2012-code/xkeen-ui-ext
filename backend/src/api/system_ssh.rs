use axum::extract::{Query, State};
use axum::response::{IntoResponse, Json, Response};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{
    api_err, api_err_detailed, api_ok, emergency_cleanup_full_tunnel_rules,
    resolve_active_dns_port, sync_full_tunnel_modes, sync_zapret_files,
};
use crate::{config, log_e, log_i, log_w, mihomo, rci, transaction::ConfigTx, AppState};
// =====================================================================
// 3. POST /api/config/apply & confirm & rollback (Commit-Confirmed)
// =====================================================================
#[derive(Debug, Deserialize)]
pub struct ApplyConfigRequest {
    #[serde(default = "default_rollback_seconds")]
    pub rollback_seconds: u64,
    pub config_yaml: Option<String>,
    pub config_json: Option<config::AppConfig>,
}

fn default_rollback_seconds() -> u64 {
    30
}

pub async fn apply_config_tx(
    State(state): State<AppState>,
    Json(body): Json<ApplyConfigRequest>,
) -> Response {
    let json_val = body.config_json.as_ref().and_then(|c| serde_json::to_value(c).ok());
    let val = crate::transaction::validate_config_dry_run(
        body.config_yaml.as_deref(),
        json_val.as_ref(),
    );
    if !val.valid {
        return api_err_detailed(
            "CONFIG_VALIDATION_FAILED",
            format!("Ошибка валидации конфигурации: {}", val.errors.join("; ")),
            Some(json!({ "errors": val.errors, "warnings": val.warnings })),
            Some("Исправьте синтаксис или устраните конфликт портов перед применением"),
        );
    }

    match crate::transaction::apply_with_confirmation(
        &state,
        body.rollback_seconds,
        body.config_yaml,
        body.config_json,
    ).await {
        Ok(tx_id) => api_ok(json!({
            "applied": true,
            "tx_id": tx_id,
            "rollback_seconds": body.rollback_seconds,
            "message": if body.rollback_seconds > 0 {
                format!("Конфигурация успешно применена. Ожидается подтверждение в течение {} сек через POST /api/config/confirm", body.rollback_seconds)
            } else {
                "Конфигурация успешно зафиксирована на постоянной основе".to_string()
            }
        })),
        Err(e) => api_err_detailed("APPLY_FAILED", e, None, Some("Проверьте права доступа и доступность ядра")),
    }
}

#[derive(Debug, Deserialize)]
pub struct ConfirmConfigRequest {
    pub tx_id: String,
}

pub async fn confirm_config_tx(
    Json(body): Json<ConfirmConfigRequest>,
) -> Response {
    match crate::transaction::confirm_commit(&body.tx_id) {
        Ok(()) => api_ok(json!({
            "confirmed": true,
            "tx_id": body.tx_id,
            "message": "Конфигурация успешно подтверждена и зафиксирована на постоянной основе",
        })),
        Err(e) => api_err_detailed("CONFIRM_FAILED", e, None, Some("Возможно, транзакция уже завершилась или истекло время ожидания")),
    }
}

pub async fn rollback_config_tx(
    State(state): State<AppState>,
    Json(body): Json<ConfirmConfigRequest>,
) -> Response {
    match crate::transaction::rollback_pending_commit(&state, &body.tx_id).await {
        Ok(()) => api_ok(json!({
            "rolled_back": true,
            "tx_id": body.tx_id,
            "message": "Конфигурация успешно откатана к исходному снимку",
        })),
        Err(e) => api_err_detailed("ROLLBACK_FAILED", e, None, None),
    }
}

// =====================================================================
// 4. POST /api/config/validate — Dry-Run атомарная валидация
// =====================================================================
#[derive(Debug, Deserialize)]
pub struct ValidateConfigPayload {
    pub config_yaml: Option<String>,
    pub config_json: Option<serde_json::Value>,
}

pub async fn validate_config(
    Json(body): Json<ValidateConfigPayload>,
) -> Response {
    let result = crate::transaction::validate_config_dry_run(
        body.config_yaml.as_deref(),
        body.config_json.as_ref(),
    );
    api_ok(serde_json::to_value(result).unwrap_or_default()).into_response()
}

/// Единый перечень секретных полей конфигурации (баг 141)
pub const SECRET_FIELDS: &[&str] = &[
    "notifications.telegram_bot_token",
    "notifications.webhook_url",
    "vkturn.vk_token",
    "vkturn.profile_token",
    "vkturn.password",
    "adguard.password",
    "rci.password",
    "rci.token",
    "mihomo.secret",
    "antigravity.own_proxy",
];

/// Единый перечень известных масок для секретных полей
pub const SECRET_MASKS: &[&str] = &["******", "vk1.a.***", "[REDACTED]"];

/// Проверяет, является ли значение маской секрета или пустой строкой (там где пустая = не менять)
pub fn is_secret_mask(val: &str, allow_empty: bool) -> bool {
    let t = val.trim();
    if allow_empty && t.is_empty() {
        return true;
    }
    SECRET_MASKS.iter().any(|&m| t == m)
}

/// Маскирование секретных полей конфигурации при отдаче в UI
pub fn mask_settings_for_api(cfg: &config::AppConfig) -> serde_json::Value {
    let mut val = serde_json::to_value(cfg).unwrap_or_default();

    if let Some(auth) = val.get_mut("auth") {
        if let Some(pw) = auth.get_mut("password_hash") {
            if !pw.as_str().unwrap_or("").is_empty() {
                *pw = serde_json::Value::String("******".into());
            }
        }
        if let Some(salt) = auth.get_mut("salt") {
            if !salt.as_str().unwrap_or("").is_empty() {
                *salt = serde_json::Value::String("******".into());
            }
        }
        if let Some(sec) = auth.get_mut("session_secret") {
            if !sec.as_str().unwrap_or("").is_empty() {
                *sec = serde_json::Value::String("******".into());
            }
        }
    }
    if let Some(rci) = val.get_mut("rci") {
        if let Some(pw) = rci.get_mut("password") {
            if !pw.as_str().unwrap_or("").is_empty() {
                *pw = serde_json::Value::String("******".into());
            }
        }
        if let Some(tok) = rci.get_mut("token") {
            if !tok.as_str().unwrap_or("").is_empty() {
                *tok = serde_json::Value::String("******".into());
            }
        }
    }
    if let Some(mihomo) = val.get_mut("mihomo") {
        if let Some(sec) = mihomo.get_mut("secret") {
            if !sec.as_str().unwrap_or("").is_empty() {
                *sec = serde_json::Value::String("******".into());
            }
        }
    }
    if let Some(adguard) = val.get_mut("adguard") {
        if let Some(pw) = adguard.get_mut("password") {
            if !pw.as_str().unwrap_or("").is_empty() {
                *pw = serde_json::Value::String("******".into());
            }
        }
    }
    if let Some(notifications) = val.get_mut("notifications") {
        if let Some(tok) = notifications.get_mut("telegram_bot_token") {
            if !tok.as_str().unwrap_or("").is_empty() {
                *tok = serde_json::Value::String("******".into());
            }
        }
        if let Some(wh) = notifications.get_mut("webhook_url") {
            if !wh.as_str().unwrap_or("").is_empty() {
                *wh = serde_json::Value::String("******".into());
            }
        }
    }
    if let Some(antigravity) = val.get_mut("antigravity") {
        if let Some(proxy) = antigravity.get_mut("own_proxy") {
            let p_str = proxy.as_str().unwrap_or("");
            // Маскируем если не пустой или если содержит учетные данные / секреты
            if !p_str.is_empty() {
                *proxy = serde_json::Value::String("******".into());
            }
        }
    }
    if let Some(vkturn) = val.get_mut("vkturn") {
        if let Some(pw) = vkturn.get_mut("password") {
            if !pw.as_str().unwrap_or("").is_empty() {
                *pw = serde_json::Value::String("******".into());
            }
        }
        if let Some(tok) = vkturn.get_mut("vk_token") {
            if let Some(s) = tok.as_str() {
                if !s.is_empty() {
                    *tok = serde_json::Value::String(crate::vkturn::mask_vk_token(s));
                }
            }
        }
        if let Some(tok) = vkturn.get_mut("profile_token") {
            if tok.as_str().is_some_and(|s| !s.is_empty()) {
                *tok = serde_json::Value::String("[REDACTED]".into());
            }
        }
    }

    val
}

/// GET /api/settings — получение настроек панели с маскированием конфиденциальных данных
pub async fn get_settings(State(state): State<AppState>) -> impl IntoResponse {
    let cfg = state.config.read().await.clone();
    let val = mask_settings_for_api(&cfg);
    api_ok(val)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingsEffect {
    LoggerLevel,
    RoutingYaml,
    CanaryRouting,
    IptablesTunnelModes,
    AdGuardSync,
    RestartRequired(&'static str),
}

/// Определяет перечень подсистем, которые необходимо применить после изменения настроек.
pub fn settings_change_effects(old: &config::AppConfig, new: &config::AppConfig) -> Vec<SettingsEffect> {
    let mut effects = Vec::new();
    if old.logs.level != new.logs.level {
        effects.push(SettingsEffect::LoggerLevel);
    }
    if old.logs.remote_syslog != new.logs.remote_syslog {
        effects.push(SettingsEffect::RestartRequired("logs.remote_syslog"));
    }
    if old.mihomo.config_path != new.mihomo.config_path {
        effects.push(SettingsEffect::RestartRequired("mihomo.config_path"));
    }
    if old.system.mihomo_bin != new.system.mihomo_bin {
        effects.push(SettingsEffect::RestartRequired("system.mihomo_bin"));
    }
    if old.failover.enabled != new.failover.enabled {
        effects.push(SettingsEffect::CanaryRouting);
    }
    if old.direct_domains != new.direct_domains
        || old.force_domains != new.force_domains
        || old.adblock_enabled != new.adblock_enabled
        || old.ignore_servers != new.ignore_servers
        || old.disabled_providers != new.disabled_providers
        || old.device_routing != new.device_routing
        || old.device_domain_rules != new.device_domain_rules
        || old.schedules != new.schedules
    {
        effects.push(SettingsEffect::RoutingYaml);
    }
    if old.whitelist != new.whitelist
        || old.gaming != new.gaming
        || old.zapret != new.zapret
    {
        effects.push(SettingsEffect::IptablesTunnelModes);
    }
    if old.adguard != new.adguard {
        effects.push(SettingsEffect::AdGuardSync);
    }
    effects
}

pub async fn put_settings(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> axum::response::Response {
    let cfg_guard = state.config_lock.lock().await;
    let current = (**state.config.read().await).clone();
    let new_cfg = match merge_settings_update(&current, &body) {
        Ok(c) => c,
        Err(e) => return api_err(e),
    };
    let effects = settings_change_effects(&current, &new_cfg);
    let needs_routing_yaml = effects.contains(&SettingsEffect::RoutingYaml);

    let mut errors: Vec<String> = Vec::new();
    let mut pending: Vec<String> = Vec::new();

    if needs_routing_yaml {
        drop(cfg_guard);
        let new_cfg_for_tx = new_cfg.clone();
        if let Err(e) = crate::api::commit_config_with_routing(&state, move |cfg| {
            *cfg = new_cfg_for_tx;
        })
        .await
        {
            return api_err(format!("Ошибка сохранения и применения маршрутов: {e}"));
        }
    } else {
        if let Err(e) = config::save(&state.config_path, &new_cfg).await {
            return api_err(format!("Ошибка сохранения конфига: {}", e));
        }
        *state.config.write().await = std::sync::Arc::new(new_cfg.clone());
        drop(cfg_guard);
    }

    for eff in &effects {
        match eff {
            SettingsEffect::LoggerLevel => {
                crate::logger::set_level(&new_cfg.logs.level);
            }
            SettingsEffect::CanaryRouting => {
                reapply_canary_routing(&state).await;
            }
            SettingsEffect::IptablesTunnelModes => {
                let yaml = tokio::fs::read_to_string(&new_cfg.mihomo.config_path)
                    .await
                    .unwrap_or_default();
                let ports = crate::routing::redir_ports(&yaml);
                let (redir_port, tproxy_port) = (ports.redir, ports.tproxy);
                let dns_port = crate::routing::mihomo_dns_port(&new_cfg);
                crate::api::routing::sync_full_tunnel_modes(&new_cfg, redir_port, tproxy_port, dns_port).await;
            }
            SettingsEffect::AdGuardSync => {
                let res = if new_cfg.adguard.enabled {
                    let active_port = crate::adguard::detect_adguard_dns_port()
                        .unwrap_or(new_cfg.adguard.dns_port);
                    crate::adguard::apply_adguard_iptables(active_port)
                } else {
                    crate::adguard::remove_adguard_iptables()
                };
                if let Err(e) = res {
                    errors.push(format!("AdGuard DNS редирект: {e}"));
                }
            }
            SettingsEffect::RoutingYaml => {}
            SettingsEffect::RestartRequired(field) => {
                pending.push(format!("Требуется перезапуск панели для применения параметра {field}"));
            }
        }
    }

    log_i!(
        "Настройки сохранены (effects: {}, errors: {}, pending: {})",
        effects.len(),
        errors.len(),
        pending.len()
    );
    let applied = errors.is_empty() && pending.is_empty();
    api_ok(json!({
        "saved": true,
        "applied": applied,
        "pending": pending,
        "errors": errors,
    }))
}

/// Мерж частичного обновления настроек поверх текущей конфигурации.
/// Раздел `auth` (хеш пароля, соль, секрет сессий, флаг включения) через общие
/// настройки менять НЕЛЬЗЯ: раньше любой запрос PUT /api/settings мог подменить
/// хеш и session_secret в обход проверки текущего пароля (A-02) и захватить панель.
/// Менять пароль/авторизацию можно только через /api/auth/change-password.
pub fn merge_settings_update(current: &config::AppConfig, body: &serde_json::Value) -> Result<config::AppConfig, String> {
    let mut merged = serde_json::to_value(current).unwrap_or_default();
    let mut patch = body.clone();
    if let Some(obj) = patch.as_object_mut() {
        obj.remove("auth");
    }
    crate::config::merge_value(&mut merged, &patch);
    let mut new_cfg: config::AppConfig =
        serde_json::from_value(merged).map_err(|e| format!("Некорректные настройки: {}", e))?;
    new_cfg.auth = current.auth.clone();
    new_cfg.failover.migrate_priority();

    // Восстановление замаскированных секретов (баг 141)
    if is_secret_mask(&new_cfg.notifications.telegram_bot_token, false) {
        new_cfg.notifications.telegram_bot_token = current.notifications.telegram_bot_token.clone();
    }
    if is_secret_mask(&new_cfg.notifications.webhook_url, false) {
        new_cfg.notifications.webhook_url = current.notifications.webhook_url.clone();
    }
    if is_secret_mask(&new_cfg.rci.password, false) {
        new_cfg.rci.password = current.rci.password.clone();
    }
    if is_secret_mask(&new_cfg.rci.token, false) {
        new_cfg.rci.token = current.rci.token.clone();
    }
    if is_secret_mask(&new_cfg.mihomo.secret, false) {
        new_cfg.mihomo.secret = current.mihomo.secret.clone();
    }
    if let Some(ref pw) = new_cfg.adguard.password {
        if is_secret_mask(pw, true) {
            new_cfg.adguard.password = current.adguard.password.clone();
        }
    }
    if is_secret_mask(&new_cfg.vkturn.password, false) {
        new_cfg.vkturn.password = current.vkturn.password.clone();
    }
    if let Some(ref tok) = new_cfg.vkturn.vk_token {
        if is_secret_mask(tok, false) {
            new_cfg.vkturn.vk_token = current.vkturn.vk_token.clone();
        }
    }
    if let Some(ref tok) = new_cfg.vkturn.profile_token {
        if is_secret_mask(tok, false) {
            new_cfg.vkturn.profile_token = current.vkturn.profile_token.clone();
        }
    }
    if is_secret_mask(&new_cfg.antigravity.own_proxy, false) {
        new_cfg.antigravity.own_proxy = current.antigravity.own_proxy.clone();
    }

    // Защита от инъекций и подмены путей в system (баг 125)
    let s = &new_cfg.system;
    for (name, path) in [
        ("xkeen_init", &s.xkeen_init),
        ("xkeen_route_init", &s.xkeen_route_init),
        ("zapret_init", &s.zapret_init),
    ] {
        if !path.is_empty() && !crate::config::is_valid_init_script_path(path) {
            return Err(format!("Недопустимый путь к init-скрипту {name}: '{path}'"));
        }
    }
    if !s.xkeen_route_bin.is_empty() && !crate::config::is_valid_xkeen_bin_path(&s.xkeen_route_bin) {
        return Err(format!("Недопустимый путь к исполняемому файлу xkeen_route_bin: '{}'", s.xkeen_route_bin));
    }
    if !s.mihomo_bin.is_empty() && !crate::config::is_valid_init_script_path(&s.mihomo_bin) {
        return Err(format!("Недопустимый путь к исполняемому файлу mihomo_bin: '{}'", s.mihomo_bin));
    }

    Ok(new_cfg)
}

/// Переприменение маршрутизации после включения/выключения failover:
/// селектор '🛡️ Canary Probe' и слушатель 127.0.0.1:7894 генерируются только при
/// failover.enabled. Раньше тумблер лишь сохранял флаг — Canary-загрузка через 7894
/// падала до следующей пересборки config.yaml.
pub async fn reapply_canary_routing(state: &AppState) {
    match crate::transaction::ConfigTx::begin(state).await {
        Ok(mut tx) => {
            let Ok(yaml) = tx.read_yaml().await else { return };
            match crate::routing::apply_routing(&yaml, tx.config()) {
                Ok((new_yaml, _)) if new_yaml != yaml => {
                    if tx.set_yaml(new_yaml).is_ok() {
                        if let Err(e) = tx.commit_and_reload().await {
                            log_w!("Не удалось применить Canary-маршрутизацию после переключения failover: {e}");
                        }
                    }
                }
                Ok(_) => {}
                Err(e) => log_w!("Ошибка пересборки маршрутизации после переключения failover: {e}"),
            }
        }
        Err(e) => log_w!("Не удалось начать транзакцию конфигурации: {e}"),
    }
}

// --- Сервис XKeen и бэкапы ---

#[derive(Deserialize)]
pub struct ServiceReq {
    pub action: String,
}

/// Планирует обновлённый YAML маршрутизации при старте/рестарте XKeen.
pub fn plan_xkeen_start_yaml(cfg: &config::AppConfig, raw_yaml: &str) -> Result<String, String> {
    let (new_yaml, _) = crate::routing::apply_routing(raw_yaml, cfg)?;
    crate::transaction::validate_yaml_syntax(&new_yaml)?;
    Ok(new_yaml)
}

/// POST /api/xkeen/service — start/stop/restart/restart_all/status сервисов проксирования и обхода.
/// При stop останавливает xkeen, mihomo и zapret и очищает перехват iptables (все устройства идут напрямую без смены политики на роутере).
pub async fn xkeen_service(State(state): State<AppState>, Json(req): Json<ServiceReq>) -> Response {
    let cfg = state.config.read().await.clone();
    let action = req.action.trim().to_string();
    if !matches!(action.as_str(), "start" | "stop" | "restart" | "restart_all" | "status") {
        return api_err("Недопустимое действие (start/stop/restart/restart_all/status)");
    }
    let init_script = cfg.system.xkeen_init.trim();
    if !crate::config::is_valid_init_script_path(init_script) {
        return api_err("Недопустимый путь к init-скрипту (разрешены только пути в /opt/ и символы [a-zA-Z0-9_-./])");
    }

    if action == "stop" {
        crate::watchdog::set_service_stopped(true);
        let stop_cmd = format!(
            r#"
            sh "{init_script}" stop 2>&1 || true
            [ -x /opt/sbin/xkeen ] && /opt/sbin/xkeen -stop >/dev/null 2>&1 || true
            [ -x /opt/etc/init.d/S51zapret ] && /opt/etc/init.d/S51zapret stop >/dev/null 2>&1 || true
            killall -15 mihomo xray nfqws nfqws2 2>/dev/null || true
            sleep 1
            killall -9 mihomo xray nfqws nfqws2 2>/dev/null || true
            for ipt in "iptables -w 2" "ip6tables -w 2"; do
              for tbl in nat mangle; do
                while $ipt -t $tbl -D PREROUTING -j xkeen 2>/dev/null; do :; done
                while $ipt -t $tbl -D OUTPUT -j xkeen_mask 2>/dev/null; do :; done
                while $ipt -t $tbl -D POSTROUTING -j zapret 2>/dev/null; do :; done
                while $ipt -t $tbl -D PREROUTING -j zapret 2>/dev/null; do :; done
                while $ipt -t $tbl -D OUTPUT -j zapret 2>/dev/null; do :; done
                while $ipt -t $tbl -D PREROUTING -j XKEEN_GAMING 2>/dev/null; do :; done
                $ipt -t $tbl -F xkeen 2>/dev/null || true
                $ipt -t $tbl -F xkeen_mask 2>/dev/null || true
                $ipt -t $tbl -F zapret 2>/dev/null || true
                $ipt -t $tbl -F XKEEN_GAMING 2>/dev/null || true
                $ipt -t $tbl -X XKEEN_GAMING 2>/dev/null || true
              done
              while ip6tables -w 2 -D FORWARD -j XKEEN_GAMING 2>/dev/null; do :; done
              ip6tables -w 2 -F XKEEN_GAMING 2>/dev/null || true
              ip6tables -w 2 -X XKEEN_GAMING 2>/dev/null || true
              $ipt -w 2 -t nat -S PREROUTING 2>/dev/null | grep 'xkeen-route-zapret' | grep 'REDIRECT' | sed 's/^-A /-D /' | while read -r rule; do
                $ipt -w 2 -t nat $rule 2>/dev/null || true
              done
            done
            while ip rule del fwmark 0x111/0xff lookup 111 2>/dev/null; do :; done
            while ip -6 rule del fwmark 0x111/0xff lookup 111 2>/dev/null; do :; done
            while ip rule del fwmark 0x115 lookup 115 2>/dev/null; do :; done
            while ip rule del fwmark 0x115/0x115 lookup 115 2>/dev/null; do :; done
            while ip -6 rule del fwmark 0x115 lookup 115 2>/dev/null; do :; done
            while ip -6 rule del fwmark 0x115/0x115 lookup 115 2>/dev/null; do :; done
            ip route del local default dev lo table 115 2>/dev/null || ip route del local 0.0.0.0/0 dev lo table 115 2>/dev/null || true
            ip route flush table 115 2>/dev/null || true
            ip -6 route del local default dev lo table 115 2>/dev/null || true
            ip -6 route flush table 115 2>/dev/null || true
            rm -f /opt/etc/ndm/netfilter.d/060-xkeen-gaming.sh 2>/dev/null || true
            echo "Все службы (XKeen, Mihomo, Zapret) остановлены. Трафик устройств переведен на прямой выход (DIRECT)."
            "#
        );
        let out = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            tokio::process::Command::new("sh").arg("-c").arg(&stop_cmd).output(),
        ).await;
        // Скрипт выше снимает только XKEEN_GAMING. Цепочки XKEEN_BS (режим БС StrictXKeen)
        // и NDM-хук 061-xkeen-bs.sh оставались: весь трафик LAN уходил в мёртвые порты
        // остановленного Mihomo, а сторож при остановленной службе молчит → блэкаут сети.
        emergency_cleanup_full_tunnel_rules().await;
        log_i!("Сервисы остановлены пользователем (прямой выход в интернет активирован)");
        let (code, stdout, stderr) = match out {
            Ok(Ok(o)) => (
                o.status.code(),
                String::from_utf8_lossy(&o.stdout).to_string(),
                String::from_utf8_lossy(&o.stderr).to_string(),
            ),
            Ok(Err(e)) => (Some(0), String::new(), e.to_string()),
            Err(_) => (Some(0), String::new(), "Превышен таймаут остановки служб (15 сек)".to_string()),
        };
        return api_ok(json!({
            "code": code,
            "stdout": stdout,
            "stderr": stderr,
            "service_running": false,
            "service_stopped": true,
            "message": "Все службы остановлены. Трафик переведён на прямой выход (DIRECT)."
        }));
    }

    if action == "start" || action == "restart" || action == "restart_all" {
        crate::watchdog::set_service_stopped(false);
    }

    let script_arg = if action == "restart_all" { "restart" } else { action.as_str() };
    let out = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        tokio::process::Command::new("sh")
            .env("fd_out", "true")
            .arg(init_script)
            .arg(script_arg)
            .output(),
    )
    .await;

    match out {
        Err(_) => {
            log_w!("Таймаут (15 сек) выполнения '{} {}'", init_script, script_arg);
            api_err(format!("Превышен таймаут выполнения '{} {}'", init_script, script_arg))
        }
        Ok(Err(e)) => {
            api_err(format!("Ошибка запуска скрипта '{}': {}", init_script, e))
        }
        Ok(Ok(o)) => {
            log_i!(
                "Сервис XKeen: {} (код {})",
                action,
                o.status.code().unwrap_or(-1)
            );
            if action == "start" || action == "restart" || action == "restart_all" {
                // Восстанавливаем правила маршрутизации в config.yaml через ConfigTx и перезапускаем Zapret (если он включен)
                let path = std::path::Path::new(&cfg.mihomo.config_path);
                if path.exists() {
                    let tx_res: Result<(), String> = async {
                        let mut tx = crate::transaction::ConfigTx::begin(&state).await?;
                        let raw_yaml = tx.read_yaml().await?;
                        let new_yaml = plan_xkeen_start_yaml(tx.config(), &raw_yaml)?;
                        let tx_cfg = tx.config().clone();
                        tx.set_yaml(new_yaml.clone())?;
                        tx.commit_and_reload().await?;
                        // stop снял цепочки XKEEN_GAMING/XKEEN_BS и NDM-хуки — без повторного
                        // синка UI показывал «игровой режим/БС включён», а трафик не перехватывался
                        let ports = crate::routing::redir_ports(&new_yaml);
                        let (redir_port, tproxy_port) = (ports.redir, ports.tproxy);
                        let dns_port = resolve_active_dns_port(&tx_cfg);
                        sync_full_tunnel_modes(&tx_cfg, redir_port, tproxy_port, dns_port).await;
                        Ok(())
                    }
                    .await;
                    if let Err(e) = tx_res {
                        log_e!("Ошибка применения маршрутизации при '{}' XKeen: {}", action, e);
                        return api_err(format!("Ошибка обновления или перезагрузки конфигурации Mihomo: {}", e));
                    }
                }
                if cfg.zapret.enabled && std::path::Path::new("/opt/etc/init.d/S51zapret").exists() {
                    let _ = sync_zapret_files(&cfg.zapret).await;
                    let _ = tokio::time::timeout(
                        std::time::Duration::from_secs(8),
                        tokio::process::Command::new("sh")
                            .arg("-c")
                            .arg("exec /opt/etc/init.d/S51zapret restart </dev/null >/dev/null 2>&1")
                            .status(),
                    )
                    .await;
                }
                if cfg.vkturn.enabled {
                    let turn_ips = state.vkturn.rt.read().await.turn_ips.clone();
                    crate::vkturn::apply_exemptions(&cfg.vkturn.peer, &turn_ips).await;
                }
                if action == "restart_all" {
                    tokio::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                        let _ = tokio::process::Command::new("sh")
                            .arg("-c")
                            .arg("/opt/etc/init.d/S99xkeen-route restart >/dev/null 2>&1 &")
                            .status()
                            .await;
                    });
                }
            }
            if !o.status.success() && action != "status" && action != "restart_all" {
                let stderr = String::from_utf8_lossy(&o.stderr);
                let stdout = String::from_utf8_lossy(&o.stdout);
                let err_msg = if !stderr.trim().is_empty() {
                    stderr.trim().to_string()
                } else if !stdout.trim().is_empty() {
                    stdout.trim().to_string()
                } else {
                    format!("Процесс завершился с кодом {}", o.status.code().unwrap_or(-1))
                };
                return api_err(format!("Команда '{}' завершилась с ошибкой: {}", action, err_msg));
            }
            let msg = match action.as_str() {
                "restart_all" => "Все компоненты (XKeen, Mihomo, Zapret, Панель) успешно перезапущены!",
                "restart" => "Службы XKeen и Mihomo успешно перезапущены!",
                "start" => "Службы успешно запущены!",
                "stop" => "Все службы остановлены. Трафик переведён на прямой выход (DIRECT).",
                _ => "Команда успешно выполнена.",
            };
            api_ok(json!({
                "code": o.status.code(),
                "stdout": String::from_utf8_lossy(&o.stdout),
                "stderr": String::from_utf8_lossy(&o.stderr),
                "service_running": action != "stop",
                "service_stopped": action == "stop",
                "message": msg,
            }))
        }
    }
}


/// GET /api/antigravity/status
pub async fn get_antigravity_status(State(state): State<AppState>) -> Response {
    let status = state.antigravity.get_status().await;
    api_ok(serde_json::to_value(status).unwrap_or_default()).into_response()
}

#[derive(Deserialize)]
pub struct AntigravitySettingsReq {
    pub enabled: Option<bool>,
    pub mode: Option<String>,
    pub proxy_port: Option<u16>,
    pub proxy_enabled: Option<bool>,
    pub health_check_interval: Option<u64>,
    pub own_proxy: Option<String>,
}

/// POST /api/antigravity/settings
pub async fn set_antigravity_settings(
    State(state): State<AppState>,
    Json(req): Json<AntigravitySettingsReq>,
) -> Response {
    let mut tx = match ConfigTx::begin(&state).await {
        Ok(tx) => tx,
        Err(e) => return api_err(format!("Ошибка захвата конфигурации: {e}")).into_response(),
    };

    if let Some(enabled) = req.enabled {
        tx.config_mut().antigravity.enabled = enabled;
    }
    if let Some(mode) = req.mode {
        tx.config_mut().antigravity.mode = mode;
    }
    if let Some(port) = req.proxy_port {
        tx.config_mut().antigravity.proxy_port = port;
    }
    if let Some(pe) = req.proxy_enabled {
        tx.config_mut().antigravity.proxy_enabled = pe;
    }
    if let Some(interval) = req.health_check_interval {
        tx.config_mut().antigravity.health_check_interval = interval.max(30);
    }
    if let Some(own_proxy) = req.own_proxy {
        if let Err(e) = crate::antigravity::validate_own_proxy(&own_proxy) {
            return super::api_err_bad_request(e).into_response();
        }
        tx.config_mut().antigravity.own_proxy = own_proxy;
    }

    let proxy_port = tx.config().antigravity.proxy_port;
    let proxy_active = tx.config().antigravity.enabled && tx.config().antigravity.proxy_enabled;

    if let Err(e) = tx.commit().await {
        return api_err(format!("Ошибка сохранения: {e}")).into_response();
    }

    // Фоновое управление прокси и проверка сразу после смены настроек (BUG-02)
    let ag = state.antigravity.clone();
    tokio::spawn(async move {
        if proxy_active {
            ag.clone().restart_proxy(proxy_port).await;
        } else {
            ag.stop_proxy().await;
        }
        ag.check_and_update().await;
    });

    api_ok(json!({ "message": "Настройки Antigravity обновлены" })).into_response()
}

/// POST /api/antigravity/check
pub async fn check_antigravity(State(state): State<AppState>) -> Response {
    state.antigravity.check_and_update().await;
    let status = state.antigravity.get_status().await;
    api_ok(json!({
        "message": "Проверка Antigravity завершена",
        "status": status,
    })).into_response()
}

/// Сырой PowerShell скрипт разблокировки входа в Antigravity
pub const ANTIGRAVITY_PATCH_SCRIPT: &str = r#"[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
[Console]::InputEncoding = [System.Text.Encoding]::UTF8
$OutputEncoding = [System.Text.Encoding]::UTF8
Write-Host "=======================================================" -ForegroundColor Cyan
Write-Host "   Antigravity & Cloud Code Login Patch (xkeen route)  " -ForegroundColor Cyan
Write-Host "=======================================================" -ForegroundColor Cyan
Write-Host ""
Write-Host "[1/3] Terminating Antigravity processes..." -ForegroundColor Yellow
$procs = @("language_server", "language_server_windows_x64", "Antigravity", "Antigravity CLI")
foreach ($p in $procs) {
    Get-Process -Name $p -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
}
Start-Sleep -Milliseconds 800
Write-Host "[2/3] Searching for language_server.exe..." -ForegroundColor Yellow
$candidates = @(
    "$env:LOCALAPPDATA\Programs\antigravity\resources\bin\language_server.exe",
    "$env:LOCALAPPDATA\Programs\antigravity\resources\app\extensions\antigravity\bin\language_server.exe",
    "$env:LOCALAPPDATA\Programs\antigravity\resources\app\extensions\antigravity\bin\language_server_windows_x64.exe",
    "$env:LOCALAPPDATA\Programs\Antigravity IDE\resources\bin\language_server.exe",
    "$env:USERPROFILE\.antigravity\bin\language_server.exe"
)
$found = @()
foreach ($c in $candidates) {
    if (Test-Path $c) { $found += $c }
}
if ($found.Count -eq 0) {
    $searchDir = "$env:LOCALAPPDATA\Programs\antigravity"
    if (Test-Path $searchDir) {
        Get-ChildItem -Path $searchDir -Filter "*language_server*.exe" -Recurse -ErrorAction SilentlyContinue | ForEach-Object { $found += $_.FullName }
    }
}
$found = $found | Select-Object -Unique
if ($found.Count -eq 0) {
    Write-Host "[!] language_server.exe files not found!" -ForegroundColor Red
} else {
    Write-Host "[3/3] Patching binary signature (ineligible -> inexigible)..." -ForegroundColor Yellow
    $enc = [System.Text.Encoding]::GetEncoding(28591)
    $patchedCount = 0
    foreach ($file in $found) {
        Write-Host "  -> $file" -ForegroundColor Gray
        try {
            $bytes = [System.IO.File]::ReadAllBytes($file)
            $text = $enc.GetString($bytes)
            if ($text.Contains('ineligible')) {
                $bak = "$file.bak"
                if (-not (Test-Path $bak)) { [System.IO.File]::Copy($file, $bak) }
                $newText = $text.Replace('ineligible', 'inexigible')
                [System.IO.File]::WriteAllBytes($file, $enc.GetBytes($newText))
                Write-Host "     [OK] Successfully patched!" -ForegroundColor Green
                $patchedCount++
            } elseif ($text.Contains('inexigible')) {
                Write-Host "     [OK] Already patched (inexigible)." -ForegroundColor Yellow
                $patchedCount++
            } else {
                Write-Host "     [?] Signature 'ineligible' not found." -ForegroundColor DarkYellow
            }
        } catch {
            Write-Host "     [!] Access error: $_" -ForegroundColor Red
        }
    }
    if ($patchedCount -gt 0) {
        Write-Host ""
        Write-Host "[SUCCESS] Unlock completed! Please restart Antigravity." -ForegroundColor Green
    }
}
Write-Host ""
"#;

fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = if chunk.len() > 1 { chunk[1] as usize } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as usize } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[(n >> 18) & 63] as char);
        out.push(TABLE[(n >> 12) & 63] as char);
        if chunk.len() > 1 {
            out.push(TABLE[(n >> 6) & 63] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[n & 63] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// GET /patch — отдавать сырой PowerShell-скрипт (text/plain; charset=utf-8)
pub async fn get_antigravity_patch_script() -> impl IntoResponse {
    (
        [
            ("Content-Type", "text/plain; charset=utf-8"),
            ("Cache-Control", "no-cache, no-store, must-revalidate"),
        ],
        ANTIGRAVITY_PATCH_SCRIPT,
    )
        .into_response()
}

/// GET /api/antigravity/fix.cmd — отдавать обертку .cmd с заголовком Content-Disposition: attachment; filename="fix_antigravity.cmd"
pub async fn get_antigravity_fix_cmd() -> impl IntoResponse {
    let b64 = base64_encode(ANTIGRAVITY_PATCH_SCRIPT.as_bytes());
    let cmd = format!(
        "@echo off\r\n\
         chcp 65001 >nul\r\n\
         title Antigravity Login Fix (xkeen route)\r\n\
         powershell -NoProfile -ExecutionPolicy Bypass -Command \"[Console]::OutputEncoding=[System.Text.Encoding]::UTF8; [System.Text.Encoding]::UTF8.GetString([System.Convert]::FromBase64String('{}')) | iex\"\r\n\
         echo.\r\n\
         pause\r\n",
        b64
    );
    (
        [
            ("Content-Type", "application/x-bat; charset=utf-8"),
            (
                "Content-Disposition",
                "attachment; filename=\"fix_antigravity.cmd\"",
            ),
            ("Cache-Control", "no-cache, no-store, must-revalidate"),
        ],
        cmd,
    )
        .into_response()
}

// ==================== XBOX DNS & SMARTDNS =============

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct XboxDnsServerInfo {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub ips: Vec<String>,
    pub ipv6: Vec<String>,
    pub status: String,
    pub latency_ms: Option<u64>,
    pub is_recommended: bool,
    pub supported_features: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct XboxDnsStatusResponse {
    pub active: bool,
    pub servers: Vec<XboxDnsServerInfo>,
    pub reverse_proxy_ips: Vec<String>,
    pub targets: Vec<String>,
    pub antigravity_fallback_ready: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct XboxDnsCheckResult {
    pub server: String,
    pub ip: String,
    pub reachable: bool,
    pub latency_ms: Option<u64>,
    pub resolved_ips: Vec<String>,
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct XboxDnsCheckResponse {
    pub timestamp: String,
    pub results: Vec<XboxDnsCheckResult>,
}

/// GET /api/xbox-dns/status
pub async fn get_xbox_dns_status(State(state): State<AppState>) -> Response {
    let ag_st = state.antigravity.get_status().await;
    let servers = vec![
        XboxDnsServerInfo {
            id: "xbox-dns-primary".into(),
            name: "xbox-dns.ru (Primary)".into(),
            provider: "Selectel (Saint Petersburg, RU)".into(),
            ips: vec!["111.88.96.54".into(), "111.88.96.55".into()],
            ipv6: vec!["2a00:ab00:1233:26::50".into(), "2a00:ab00:1233:26::51".into()],
            status: "online".into(),
            latency_ms: None,
            is_recommended: true,
            supported_features: vec![
                "Xbox Live (0x80a40401 fix)".into(),
                "Xbox Game Pass".into(),
                "xCloud Gaming".into(),
                "Google AI Studio".into(),
                "Google Gemini".into(),
                "PlayStation Network".into(),
            ],
        },
        XboxDnsServerInfo {
            id: "xbox-dns-alt".into(),
            name: "xbox-dns.ru (Alt Pool)".into(),
            provider: "Selectel (Saint Petersburg, RU)".into(),
            ips: vec!["111.88.96.50".into(), "111.88.96.51".into()],
            ipv6: vec!["2a00:ab00:1233:26::50".into(), "2a00:ab00:1233:26::51".into()],
            status: "online".into(),
            latency_ms: None,
            is_recommended: false,
            supported_features: vec![
                "Xbox Live (0x80a40401 fix)".into(),
                "Xbox Game Pass".into(),
                "Google AI Studio".into(),
                "Google Gemini".into(),
            ],
        },
        XboxDnsServerInfo {
            id: "comss-one".into(),
            name: "Comss.one SmartDNS".into(),
            provider: "Comss Network".into(),
            ips: vec!["83.220.169.155".into(), "212.109.195.93".into()],
            ipv6: vec![],
            status: "online".into(),
            latency_ms: None,
            is_recommended: false,
            supported_features: vec![
                "Xbox Live".into(),
                "PlayStation Network".into(),
                "Discord".into(),
                "Google AI Studio".into(),
            ],
        },
    ];

    let resp = XboxDnsStatusResponse {
        active: ag_st.enabled,
        servers,
        reverse_proxy_ips: vec!["188.68.214.130".into(), "188.68.214.143".into()],
        targets: vec![
            "auth.xboxlive.com".into(),
            "xsts.auth.xboxlive.com".into(),
            "aistudio.google.com".into(),
            "gemini.google.com".into(),
            "generativelanguage.googleapis.com".into(),
            "cloudcode-pa.googleapis.com".into(),
        ],
        antigravity_fallback_ready: true,
    };

    Json(resp).into_response()
}

/// POST /api/xbox-dns/check
pub async fn check_xbox_dns(State(_state): State<AppState>) -> Response {
    let candidates = vec![
        ("xbox-dns.ru (Primary)", "111.88.96.54"),
        ("xbox-dns.ru (Alt)", "111.88.96.50"),
        ("comss.one", "83.220.169.155"),
    ];

    let mut results = Vec::new();

    for (name, ip) in candidates {
        let sock_res = tokio::net::UdpSocket::bind("0.0.0.0:0").await;
        match sock_res {
            Ok(sock) => {
                let target = format!("{ip}:53");
                let query = crate::antigravity::build_dns_query("aistudio.google.com", 0x7b11);
                let start = std::time::Instant::now();
                if let Err(e) = sock.send_to(&query, &target).await {
                    results.push(XboxDnsCheckResult {
                        server: name.into(),
                        ip: ip.into(),
                        reachable: false,
                        latency_ms: None,
                        resolved_ips: Vec::new(),
                        error: Some(e.to_string()),
                    });
                    continue;
                }

                let mut buf = [0u8; 1024];
                match tokio::time::timeout(std::time::Duration::from_millis(2500), sock.recv_from(&mut buf)).await {
                    Ok(Ok((len, _))) => {
                        let elapsed = start.elapsed().as_millis() as u64;
                        let ips = crate::antigravity::parse_dns_a_records(&buf[..len]);
                        results.push(XboxDnsCheckResult {
                            server: name.into(),
                            ip: ip.into(),
                            reachable: true,
                            latency_ms: Some(elapsed),
                            resolved_ips: ips.into_iter().map(|a| a.to_string()).collect(),
                            error: None,
                        });
                    }
                    Ok(Err(e)) => {
                        results.push(XboxDnsCheckResult {
                            server: name.into(),
                            ip: ip.into(),
                            reachable: false,
                            latency_ms: None,
                            resolved_ips: Vec::new(),
                            error: Some(e.to_string()),
                        });
                    }
                    Err(_) => {
                        results.push(XboxDnsCheckResult {
                            server: name.into(),
                            ip: ip.into(),
                            reachable: false,
                            latency_ms: None,
                            resolved_ips: Vec::new(),
                            error: Some("DNS query timeout".into()),
                        });
                    }
                }
            }
            Err(e) => {
                results.push(XboxDnsCheckResult {
                    server: name.into(),
                    ip: ip.into(),
                    reachable: false,
                    latency_ms: None,
                    resolved_ips: Vec::new(),
                    error: Some(e.to_string()),
                });
            }
        }
    }

    let now_str = chrono::Local::now().format("%H:%M:%S").to_string();
    Json(XboxDnsCheckResponse {
        timestamp: now_str,
        results,
    })
    .into_response()
}

/// GET /api/xbox-dns/fix.cmd — One-click setup/reset batch script for Windows clients (Strict English ASCII)
pub async fn get_xbox_dns_fix_cmd() -> impl IntoResponse {
    let script_content = "@echo off\r\n\
        setlocal EnableDelayedExpansion\r\n\
        title Xbox DNS Setup & Diagnostic Tool - xkeen route\r\n\
        chcp 437 >nul\r\n\
        echo ========================================================\r\n\
        echo    Xbox-DNS & SmartDNS Client Manager (xkeen route)    \r\n\
        echo ========================================================\r\n\
        echo.\r\n\
        echo Select an action:\r\n\
        echo   [1] Apply Xbox-DNS (Primary: 111.88.96.54, Alt: 111.88.96.55)\r\n\
        echo   [2] Reset DNS to Automatic (DHCP)\r\n\
        echo   [3] Test DNS Connectivity and Latency\r\n\
        echo   [4] Exit\r\n\
        echo.\r\n\
        set /p choice=\"Enter choice [1-4]: \"\r\n\
        if \"%choice%\"==\"1\" goto SET_DNS\r\n\
        if \"%choice%\"==\"2\" goto RESET_DNS\r\n\
        if \"%choice%\"==\"3\" goto TEST_DNS\r\n\
        if \"%choice%\"==\"4\" goto EXIT_SCRIPT\r\n\
        goto EXIT_SCRIPT\r\n\
        \r\n\
        :SET_DNS\r\n\
        echo.\r\n\
        echo [INFO] Applying Xbox-DNS servers to active network interfaces...\r\n\
        powershell -NoProfile -Command \"Get-NetAdapter | Where-Object { $_.Status -eq 'Up' } | ForEach-Object { Set-DnsClientServerAddress -InterfaceIndex $_.ifIndex -ServerAddresses ('111.88.96.54','111.88.96.55'); Write-Host ('[OK] Applied Xbox-DNS to ' + $_.Name) -ForegroundColor Green }\"\r\n\
        ipconfig /flushdns >nul\r\n\
        echo [OK] DNS cache flushed successfully.\r\n\
        echo [OK] Done! Xbox Live, Game Pass, and AI Studio bypass are now active.\r\n\
        goto END_SCRIPT\r\n\
        \r\n\
        :RESET_DNS\r\n\
        echo.\r\n\
        echo [INFO] Resetting DNS servers to automatic (DHCP)...\r\n\
        powershell -NoProfile -Command \"Get-NetAdapter | Where-Object { $_.Status -eq 'Up' } | ForEach-Object { Set-DnsClientServerAddress -InterfaceIndex $_.ifIndex -ResetServerAddresses; Write-Host ('[OK] Reset DNS on ' + $_.Name) -ForegroundColor Green }\"\r\n\
        ipconfig /flushdns >nul\r\n\
        echo [OK] DNS reset to default router DHCP.\r\n\
        goto END_SCRIPT\r\n\
        \r\n\
        :TEST_DNS\r\n\
        echo.\r\n\
        echo [INFO] Testing DNS resolution via Xbox-DNS (111.88.96.54)...\r\n\
        powershell -NoProfile -Command \"$res = Resolve-DnsName -Name auth.xboxlive.com -Server 111.88.96.54 -QuickTimeout -ErrorAction SilentlyContinue; if ($res) { Write-Host '[OK] auth.xboxlive.com resolved successfully!' -ForegroundColor Green } else { Write-Host '[WARN] DNS query timed out' -ForegroundColor Yellow }\"\r\n\
        powershell -NoProfile -Command \"$res = Resolve-DnsName -Name aistudio.google.com -Server 111.88.96.54 -QuickTimeout -ErrorAction SilentlyContinue; if ($res) { Write-Host '[OK] aistudio.google.com proxied via Selectel!' -ForegroundColor Green } else { Write-Host '[WARN] DNS query timed out' -ForegroundColor Yellow }\"\r\n\
        goto END_SCRIPT\r\n\
        \r\n\
        :END_SCRIPT\r\n\
        echo.\r\n\
        pause\r\n\
        :EXIT_SCRIPT\r\n";

    (
        [
            ("Content-Type", "application/x-bat; charset=utf-8"),
            (
                "Content-Disposition",
                "attachment; filename=\"fix_xbox_dns.cmd\"",
            ),
            ("Cache-Control", "no-cache, no-store, must-revalidate"),
        ],
        script_content,
    )
        .into_response()
}

// ==================== ВСТРОЕННЫЙ РЕДАКТОР КОНФИГОВ =============

#[derive(Deserialize)]
pub struct ConfigFileQuery {
    pub file: String,
}

#[derive(Deserialize)]
pub struct SaveConfigFileRequest {
    pub file: String,
    pub content: String,
    #[serde(default)]
    pub reload_mihomo: bool,
}

fn resolve_config_file_path(id: &str, cfg: &config::AppConfig, config_path: &str) -> Option<std::path::PathBuf> {
    let providers_dir = std::path::Path::new(&cfg.mihomo.config_path)
        .parent()
        .map(|p| p.join("providers"))
        .unwrap_or_else(|| std::path::PathBuf::from("/opt/etc/mihomo/providers"));

    match id {
        "mihomo" => Some(std::path::PathBuf::from(&cfg.mihomo.config_path)),
        "route" => Some(std::path::PathBuf::from(config_path)),
        "override" => Some(std::path::PathBuf::from(crate::override_sync::OVERRIDE_FILE)),
        "xkeen_conf" => Some(std::path::PathBuf::from(crate::override_sync::XKEEN_CONF_FILE)),
        "crontab" => Some(std::path::PathBuf::from(crate::override_sync::SYSTEM_CRONTAB_FILE)),
        "adguard" => {
            let p = std::path::PathBuf::from(&cfg.adguard.config_path);
            if p.exists() {
                Some(p)
            } else if std::path::Path::new("/opt/etc/AdGuardHome/AdGuardHome.yaml").exists() {
                Some(std::path::PathBuf::from("/opt/etc/AdGuardHome/AdGuardHome.yaml"))
            } else {
                Some(p)
            }
        }
        other => {
            if let Some(name) = other.strip_prefix("provider:") {
                if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
                    return None;
                }
                Some(providers_dir.join(format!("{}.yaml", name)))
            } else {
                None
            }
        }
    }
}

/// GET /api/config-files/list — список доступных для редактирования файлов
pub async fn list_config_files(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    let providers_dir = std::path::Path::new(&cfg.mihomo.config_path)
        .parent()
        .map(|p| p.join("providers"))
        .unwrap_or_else(|| std::path::PathBuf::from("/opt/etc/mihomo/providers"));

    let mut files = vec![
        json!({ "id": "mihomo", "name": "Mihomo Config (config.yaml)", "path": cfg.mihomo.config_path, "syntax": "yaml" }),
        json!({ "id": "route", "name": "XKeen Route Config (config.json)", "path": state.config_path.display().to_string(), "syntax": "json" }),
        json!({ "id": "override", "name": "RU Override IP List (ru_exclude_override.lst)", "path": crate::override_sync::OVERRIDE_FILE, "syntax": "text" }),
        json!({ "id": "xkeen_conf", "name": "XKeen Settings (xkeen.conf)", "path": crate::override_sync::XKEEN_CONF_FILE, "syntax": "shell" }),
        json!({ "id": "crontab", "name": "System Crontab (/opt/etc/crontab)", "path": crate::override_sync::SYSTEM_CRONTAB_FILE, "syntax": "shell" }),
    ];

    let agh_p = std::path::Path::new(&cfg.adguard.config_path);
    let agh_alt = std::path::Path::new("/opt/etc/AdGuardHome/AdGuardHome.yaml");
    if agh_p.exists() || agh_alt.exists() || cfg.adguard.enabled {
        let actual_p = if agh_p.exists() { agh_p } else { agh_alt };
        files.push(json!({
            "id": "adguard",
            "name": "AdGuard Home Config (AdGuardHome.yaml)",
            "path": actual_p.display().to_string(),
            "syntax": "yaml"
        }));
    }

    if let Ok(mut entries) = tokio::fs::read_dir(&providers_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("yaml") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    files.push(json!({
                        "id": format!("provider:{}", stem),
                        "name": format!("Provider: {}.yaml", stem),
                        "path": path.display().to_string(),
                        "syntax": "yaml"
                    }));
                }
            }
        }
    }

    api_ok(json!(files))
}

/// GET /api/config-files/read?file=mihomo
pub async fn read_config_file(
    State(state): State<AppState>,
    Query(q): Query<ConfigFileQuery>,
) -> Response {
    let cfg = state.config.read().await.clone();
    let config_path_str = state.config_path.display().to_string();
    let path = match resolve_config_file_path(&q.file, &cfg, &config_path_str) {
        Some(p) => p,
        None => return api_err("Недопустимый идентификатор файла"),
    };

    match tokio::fs::read_to_string(&path).await {
        Ok(content) => api_ok(json!({
            "file": q.file,
            "path": path.display().to_string(),
            "content": content
        })),
        Err(e) => api_err(format!("Ошибка чтения файла {}: {}", path.display(), e)),
    }
}

/// POST /api/config-files/save
pub async fn save_config_file(
    State(state): State<AppState>,
    Json(body): Json<SaveConfigFileRequest>,
) -> Response {
    let mut tx = match ConfigTx::begin(&state).await {
        Ok(tx) => tx,
        Err(e) => return api_err(e),
    };

    let config_path_str = state.config_path.display().to_string();
    let path = match resolve_config_file_path(&body.file, tx.config(), &config_path_str) {
        Some(p) => p,
        None => return api_err("Недопустимый идентификатор файла"),
    };

    if body.file == "route" {
        if let Err(e) = serde_json::from_str::<serde_json::Value>(&body.content) {
            return api_err(format!("Ошибка синтаксиса JSON в файле config.json: {e}"));
        }
        let parsed_cfg = config::parse_config_content(&body.content, &path.display().to_string());
        tx.stage_raw_json(&body.content, parsed_cfg);
    } else if body.file == "mihomo" {
        if let Err(e) = tx.set_yaml(&body.content) {
            return api_err(e);
        }
    } else {
        let is_yaml = body.file.starts_with("provider:")
            || path.extension().is_some_and(|ext| ext == "yaml" || ext == "yml");
        if let Err(e) = tx.set_extra_file(&path, &body.content, is_yaml).await {
            return api_err(e);
        }
    }

    if path.exists() {
        let bak = format!("{}.bak", path.display());
        let _ = tokio::fs::copy(&path, &bak).await;
    }

    let should_reload = body.reload_mihomo || body.file == "mihomo";
    if should_reload {
        if let Err(e) = tx.commit_and_reload().await {
            return api_err(e);
        }
    } else {
        if let Err(e) = tx.commit_without_reload().await {
            return api_err(e);
        }
    }

    if body.file == "override" {
        let _ = crate::override_sync::sync_from_config(tx.config(), &[]).await;
    }

    log_i!("Файл {} успешно сохранён через веб-редактор", path.display());
    api_ok(json!({ "saved": true, "path": path.display().to_string() }))
}


// ==================== ДИАГНОСТИКА СЕТИ & SMART DNS =============

/// Баг A-08: для URL с generate_204 успехом считается только HTTP 204 (защита от captive portal HTTP 200).
pub fn is_internet_status(url: &str, status: u16) -> bool {
    if url.contains("generate_204") {
        status == 204
    } else {
        (200..300).contains(&status)
    }
}

/// GET /api/diagnostics/health — комплексная проверка здоровья роутера и компонентов
pub async fn get_diagnostics_health(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();

    // 1. Проверка шлюза Keenetic (RCI)
    let gateway_res = rci::get_version(&state.http, &cfg).await;
    let (gw_status, gw_msg) = match gateway_res {
        Ok(v) => {
            let ver = v.get("version").cloned().unwrap_or_else(|| "доступна".into());
            ("ok", format!("KeeneticOS {}", ver))
        }
        Err(e) => ("fail", format!("Keenetic RCI недоступен: {}", e)),
    };

    // 2. Проверка ядра Mihomo
    let mihomo_res = mihomo::get_version(&state.http, &cfg).await;
    let (mihomo_status, mihomo_msg) = match mihomo_res {
        Some(v) => ("ok", format!("Mihomo {} работает штатно", v)),
        None => ("fail", "Mihomo API недоступен".into()),
    };

    // 3. Проверка DNS резолва
    let start_dns = std::time::Instant::now();
    let dns_ok = tokio::net::lookup_host("google.com:80").await.is_ok();
    let dns_ms = start_dns.elapsed().as_millis();
    let (dns_status, dns_msg) = if dns_ok {
        ("ok", format!("DNS резолв успешен ({} мс)", dns_ms))
    } else {
        ("fail", "DNS резолв не удался".to_string())
    };

    // 4. Проверка WAN доступа
    let wan_url = "http://cp.cloudflare.com/generate_204";
    let wan_ok = state.http.get(wan_url)
        .timeout(std::time::Duration::from_secs(3))
        .send().await.map(|r| is_internet_status(wan_url, r.status().as_u16())).unwrap_or(false);
    let (wan_status, wan_msg) = if wan_ok {
        ("ok", "Интернет-соединение (WAN) активно".to_string())
    } else {
        ("warn", "Прямой доступ к тестовому узлу не отвечает".to_string())
    };

    // 5. Проверка хранилища /opt (DIAG-03: честный статус вместо безусловного ok)
    let (disk_status, disk_msg) = match tokio::process::Command::new("df").arg("-h").arg("/opt").output().await {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            // Безопасный поиск процента использования: пропускаем заголовок и ищем токен, оканчивающийся на %
            let use_percent = stdout.lines()
                .skip(1)
                .flat_map(|l| l.split_whitespace())
                .find_map(|p| p.strip_suffix('%').and_then(|s| s.parse::<u8>().ok()))
                .unwrap_or(0);
            let detail_line = stdout.lines().skip(1).collect::<Vec<_>>().join(" ");
            let display_text = if detail_line.trim().is_empty() {
                "Накопитель Entware смонтирован".to_string()
            } else {
                detail_line.trim().to_string()
            };
            if use_percent >= 98 {
                ("fail", format!("Накопитель /opt заполнен на {}%! ({})", use_percent, display_text))
            } else if use_percent >= 90 {
                ("warn", format!("Накопитель /opt заполнен на {}% ({})", use_percent, display_text))
            } else {
                ("ok", display_text)
            }
        }
        Ok(out) => ("warn", format!("Команда df вернула ошибку: {}", out.status)),
        Err(e) => ("unknown", format!("Проверка дискового пространства недоступна: {}", e)),
    };

    api_ok(json!({
        "checks": [
            { "id": "gateway", "name": "Шлюз роутера Keenetic", "status": gw_status, "message": gw_msg, "measurement_point": "router_local" },
            { "id": "mihomo", "name": "Ядро Mihomo (XKeen)", "status": mihomo_status, "message": mihomo_msg, "measurement_point": "router_local" },
            { "id": "dns", "name": "DNS Резолвер", "status": dns_status, "message": dns_msg, "latency_ms": dns_ms, "measurement_point": "router_local" },
            { "id": "wan", "name": "Прямой выход в интернет", "status": wan_status, "message": wan_msg, "measurement_point": "router_wan" },
            { "id": "storage", "name": "Дисковое пространство /opt", "status": disk_status, "message": disk_msg, "measurement_point": "router_local" },
        ]
    }))
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regression_bug_126b_put_settings_reports_real_effects() {
        let base = config::AppConfig::default();
        assert!(settings_change_effects(&base, &base).is_empty());

        let mut routing_changed = base.clone();
        routing_changed.direct_domains.push("example.com".to_string());
        let eff_routing = settings_change_effects(&base, &routing_changed);
        assert!(eff_routing.contains(&SettingsEffect::RoutingYaml));

        let mut tunnel_changed = base.clone();
        tunnel_changed.gaming.enabled = !base.gaming.enabled;
        let eff_tunnel = settings_change_effects(&base, &tunnel_changed);
        assert!(eff_tunnel.contains(&SettingsEffect::IptablesTunnelModes));

        let mut restart_changed = base.clone();
        restart_changed.logs.remote_syslog = "192.168.1.50:514".to_string();
        let eff_restart = settings_change_effects(&base, &restart_changed);
        assert!(eff_restart.contains(&SettingsEffect::RestartRequired("logs.remote_syslog")));
    }
}
