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
        || old.whitelist != new.whitelist
        || old.gaming != new.gaming
        || old.zapret != new.zapret
        || old.flow_server != new.flow_server
    {
        effects.push(SettingsEffect::RoutingYaml);
    }
    if old.whitelist != new.whitelist
        || old.gaming != new.gaming
        || old.zapret != new.zapret
    {
        effects.push(SettingsEffect::IptablesTunnelModes);
    }
    if old.adguard != new.adguard || old.adblock_enabled != new.adblock_enabled {
        effects.push(SettingsEffect::AdGuardSync);
    }
    effects
}

pub async fn put_settings(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> axum::response::Response {
    // R07: выполняем merge и commit внутри одной транзакции ConfigTx без сброса блокировки,
    // чтобы параллельное изменение (например, change-password) не затиралось устаревшим снимком.
    let mut tx = match crate::transaction::ConfigTx::begin(&state).await {
        Ok(t) => t,
        Err(e) => return api_err(e),
    };
    let old_cfg = tx.config().clone();
    let new_cfg = match merge_settings_update(&old_cfg, &body) {
        Ok(c) => c,
        Err(e) => return api_err(e),
    };
    let effects = settings_change_effects(&old_cfg, &new_cfg);
    let needs_routing_yaml = effects.contains(&SettingsEffect::RoutingYaml);
    *tx.config_mut() = new_cfg.clone();

    let mut errors: Vec<String> = Vec::new();
    let mut pending: Vec<String> = Vec::new();

    if needs_routing_yaml {
        let yaml = match tx.read_yaml().await {
            Ok(y) => y,
            Err(e) => return api_err(format!("Ошибка чтения YAML маршрутов: {e}")),
        };
        let (new_yaml, _) = match crate::routing::apply_routing(&yaml, &new_cfg) {
            Ok(res) => res,
            Err(e) => return api_err(format!("Ошибка применения маршрутов: {e}")),
        };
        if let Err(e) = tx.set_yaml(new_yaml) {
            return api_err(format!("Ошибка подготовки YAML маршрутов: {e}"));
        }
        if let Err(e) = tx.commit_and_reload().await {
            return api_err(format!("Ошибка сохранения и применения маршрутов: {e}"));
        }
    } else if let Err(e) = tx.commit_without_reload().await {
        return api_err(format!("Ошибка сохранения конфига: {}", e));
    }
    drop(tx);

    for eff in &effects {
        match eff {
            SettingsEffect::LoggerLevel => {
                crate::logger::set_level(&new_cfg.logs.level);
            }
            SettingsEffect::RestartRequired("logs.remote_syslog") => {
                crate::logger::reconfigure_syslog(&new_cfg.logs.remote_syslog);
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
                // R09: используем единое определение активного DNS (с учётом AdGuard)
                let dns_port = resolve_active_dns_port(&new_cfg);
                if let Err(e) = crate::api::routing::sync_full_tunnel_modes(&new_cfg, redir_port, tproxy_port, dns_port).await {
                    errors.push(format!("iptables (полный перехват): {e}"));
                }
            }
            SettingsEffect::AdGuardSync => {
                // R10: при изменении adblock_enabled через /api/settings синхронизируем защиту AdGuard
                if old_cfg.adblock_enabled != new_cfg.adblock_enabled {
                    if let Err(e) = crate::adguard::set_protection(&state.http, &new_cfg.adguard, new_cfg.adblock_enabled).await {
                        errors.push(format!("AdGuard защита: {e}"));
                    }
                }
                let res = if new_cfg.adguard.enabled || new_cfg.adblock_enabled {
                    let active_port = crate::adguard::detect_adguard_dns_port()
                        .unwrap_or(new_cfg.adguard.dns_port);
                    crate::adguard::apply_adguard_iptables(active_port)
                } else {
                    crate::adguard::remove_adguard_iptables_for_mihomo_config(&new_cfg.mihomo.config_path)
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

    crate::api::routing::devices::validate_schedules(&new_cfg.schedules)?;

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
            IPT_W=""; command iptables -w -L -n >/dev/null 2>&1 && IPT_W="-w"
            IP6T_W=""; command ip6tables -w -L -n >/dev/null 2>&1 && IP6T_W="-w"
            for ipt in "iptables $IPT_W" "ip6tables $IP6T_W"; do
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
              while ip6tables $IP6T_W -D FORWARD -j XKEEN_GAMING 2>/dev/null; do :; done
              ip6tables $IP6T_W -F XKEEN_GAMING 2>/dev/null || true
              ip6tables $IP6T_W -X XKEEN_GAMING 2>/dev/null || true
              $ipt -t nat -S PREROUTING 2>/dev/null | grep 'xkeen-route-zapret' | grep 'REDIRECT' | sed 's/^-A /-D /' | while read -r rule; do
                $ipt -t nat $rule 2>/dev/null || true
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
    let is_start_action = matches!(action.as_str(), "start" | "restart" | "restart_all");
    let mihomo_was_running = crate::system::is_process_running("mihomo");

    // Скрипт xkeen при запуске вызывает /opt/etc/init.d/S05xkeen и при его отсутствии
    // молча выходит с кодом 0, не запуская ядро. Поэтому при отсутствии init-скрипта
    // (или если он не поднял процесс) запускаем mihomo напрямую.
    let fallback_mihomo_cmd = r#"
killall -15 mihomo 2>/dev/null || true
sleep 1
if command -v mihomo >/dev/null 2>&1; then
  nohup mihomo -d /opt/etc/mihomo </dev/null >/opt/var/log/mihomo.log 2>&1 &
elif [ -x /opt/sbin/mihomo ]; then
  nohup /opt/sbin/mihomo -d /opt/etc/mihomo </dev/null >/opt/var/log/mihomo.log 2>&1 &
elif [ -x /opt/bin/mihomo ]; then
  nohup /opt/bin/mihomo -d /opt/etc/mihomo </dev/null >/opt/var/log/mihomo.log 2>&1 &
else
  echo "Ни S05xkeen, ни бинарник mihomo не найдены" >&2
  exit 1
fi
"#;

    let out = if std::path::Path::new(init_script).exists() {
        let mut cmd = tokio::process::Command::new("sh");
        cmd.env("fd_out", "true").arg(init_script).arg(script_arg);
        if is_start_action {
            cmd.arg("on");
        }
        tokio::time::timeout(
            std::time::Duration::from_secs(15),
            cmd.output(),
        )
        .await
    } else if is_start_action {
        // AUDIT-080 / AUDIT-122: fallback если S05xkeen отсутствует (как на стенде WBR3000UAX)
        tokio::time::timeout(
            std::time::Duration::from_secs(15),
            tokio::process::Command::new("sh").arg("-c").arg(fallback_mihomo_cmd).output(),
        )
        .await
    } else {
        return api_err(format!("Init-скрипт '{}' не найден", init_script));
    };

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
                        if action == "start" || !mihomo_was_running {
                            tx.commit_without_reload().await?;
                        } else {
                            tx.commit_and_reload().await?;
                        }
                        // stop снял цепочки XKEEN_GAMING/XKEEN_BS и NDM-хуки — без повторного
                        // синка UI показывал «игровой режим/БС включён», а трафик не перехватывался
                        let ports = crate::routing::redir_ports(&new_yaml);
                        let (redir_port, tproxy_port) = (ports.redir, ports.tproxy);
                        let dns_port = resolve_active_dns_port(&tx_cfg);
                        sync_full_tunnel_modes(&tx_cfg, redir_port, tproxy_port, dns_port).await?;
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
            if is_start_action {
                let mut started = false;
                for _ in 0..10 {
                    if crate::system::is_process_running("mihomo") {
                        started = true;
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                }
                if !started {
                    log_w!("После '{}' процесс mihomo не обнаружен — пробуем прямой fallback-запуск", action);
                    let _ = tokio::time::timeout(
                        std::time::Duration::from_secs(10),
                        tokio::process::Command::new("sh").arg("-c").arg(fallback_mihomo_cmd).output(),
                    )
                    .await;
                    for _ in 0..10 {
                        if crate::system::is_process_running("mihomo") {
                            started = true;
                            break;
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    }
                }
                if !started {
                    let log_tail = tokio::fs::read_to_string("/opt/var/log/mihomo.log")
                        .await
                        .ok()
                        .map(|s| {
                            s.lines()
                                .rev()
                                .take(5)
                                .collect::<Vec<_>>()
                                .into_iter()
                                .rev()
                                .collect::<Vec<_>>()
                                .join(" | ")
                        })
                        .unwrap_or_default();
                    let detail = if log_tail.is_empty() {
                        "Проверьте наличие бинарника mihomo и конфигурацию /opt/etc/mihomo/config.yaml".to_string()
                    } else {
                        format!("Лог: {}", log_tail)
                    };
                    return api_err(format!("Не удалось запустить процесс mihomo ({}). {}", action, detail));
                }
            }
            let running_now = if is_start_action {
                true
            } else if action == "stop" {
                false
            } else {
                crate::system::is_process_running("mihomo")
            };
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
                "service_running": running_now,
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
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(input)
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
        Ok(content) => {
            let safe_content = if q.file == "route" {
                crate::api::backups::redact_route_config_json(&content)
            } else {
                content
            };
            api_ok(json!({
                "file": q.file,
                "path": path.display().to_string(),
                "content": safe_content
            }))
        }
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
        if let Err(e) = serde_json::from_str::<config::AppConfig>(&body.content) {
            return api_err(format!("Некорректная структура конфигурации в файле config.json: {e}"));
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
            ("ok", if ver.starts_with("KeeneticOS") { ver } else { format!("KeeneticOS {}", ver) })
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


/// POST /api/settings/reset — полный сброс настроек панели к значениям по умолчанию (с сохранением auth).
pub async fn reset_settings(State(state): State<AppState>) -> Response {
    crate::free_sub::reset_free_sub_runtime_and_cache(&state).await;

    let pre_cfg = state.config.read().await.clone();
    let mihomo_online = mihomo::get_version(&state.http, &pre_cfg).await.is_some();

    let mut tx = match ConfigTx::begin(&state).await {
        Ok(tx) => tx,
        Err(e) => return api_err(e),
    };
    let old_mihomo_path = tx.config().mihomo.config_path.clone();
    let old_mihomo_host = tx.config().mihomo.host.clone();
    let old_mihomo_port = tx.config().mihomo.port;
    let old_adguard_path = tx.config().adguard.config_path.clone();
    let old_xkeen_init = tx.config().system.xkeen_init.clone();
    let old_auth = tx.config().auth.clone();

    let mut defaults = config::AppConfig::default();
    if !old_mihomo_path.is_empty() {
        defaults.mihomo.config_path = old_mihomo_path;
    }
    if !old_mihomo_host.is_empty() {
        defaults.mihomo.host = old_mihomo_host;
    }
    if old_mihomo_port > 0 {
        defaults.mihomo.port = old_mihomo_port;
    }
    if !old_adguard_path.is_empty() {
        defaults.adguard.config_path = old_adguard_path;
    }
    if !old_xkeen_init.is_empty() {
        defaults.system.xkeen_init = old_xkeen_init;
    }
    defaults.auth = old_auth;
    if !super::is_nfqws1_available() && !super::is_nfqws2_available() {
        defaults.zapret.enabled = false;
    }
    *tx.config_mut() = defaults;

    let mut ports = crate::routing::redir_ports("");
    if let Ok(mut yaml) = tx.read_yaml().await {
        for pid in [
            crate::free_sub::FREE_WL_PROVIDER_ID,
            crate::free_sub::FREE_VPN_PROVIDER_ID,
            crate::routing::FREE_SUB_PROBE_PROVIDER_ID,
        ] {
            if let Ok(cleaned) = crate::routing::delete_provider_from_yaml(&yaml, pid) {
                yaml = cleaned;
            }
        }
        ports = crate::routing::redir_ports(&yaml);
        if let Ok((new_yaml, _)) = crate::routing::apply_routing(&yaml, tx.config()) {
            ports = crate::routing::redir_ports(&new_yaml);
            let _ = tx.set_yaml(new_yaml);
        }
    }
    let reset_cfg = tx.config().clone();
    let commit_res = if mihomo_online {
        tx.commit_and_reload().await
    } else {
        tx.commit_without_reload().await
    };
    if let Err(e) = commit_res {
        return api_err(format!("Ошибка сброса настроек: {e}"));
    }
    drop(tx);

    if let Some(prov_dir) = std::path::Path::new(&reset_cfg.mihomo.config_path)
        .parent()
        .map(|p| p.join("providers"))
    {
        for pid in [
            crate::free_sub::FREE_WL_PROVIDER_ID,
            crate::free_sub::FREE_VPN_PROVIDER_ID,
            crate::routing::FREE_SUB_PROBE_PROVIDER_ID,
        ] {
            let _ = tokio::fs::remove_file(prov_dir.join(format!("{pid}.yaml"))).await;
        }
    }

    if mihomo_online {
        mihomo::invalidate_proxies_cache();
        let _ = mihomo::switch_server(&state.http, &reset_cfg, "DIRECT").await;
    }

    let dns_port = resolve_active_dns_port(&reset_cfg);
    if let Err(e) = sync_full_tunnel_modes(&reset_cfg, ports.redir, ports.tproxy, dns_port).await {
        log_w!("[RESET] Предупреждение синхронизации режимов при сбросе: {e}");
    }
    crate::traffic::set_whitelist_active(false);
    crate::free_sub::reset_free_sub_runtime_and_cache(&state).await;
    log_i!("Настройки панели полностью сброшены к значениям по умолчанию (auth сохранён)");
    api_ok(json!({ "reset": true }))
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

        // R08: whitelist/gaming/zapret/flow_server должны назначать RoutingYaml
        let eff_wl = settings_change_effects(&base, &tunnel_changed);
        assert!(eff_wl.contains(&SettingsEffect::RoutingYaml));
        let mut flow_changed = base.clone();
        flow_changed.flow_server = Some("VPS-1".to_string());
        assert!(settings_change_effects(&base, &flow_changed).contains(&SettingsEffect::RoutingYaml));

        // R10: adblock_enabled должен назначать AdGuardSync
        let mut adblock_changed = base.clone();
        adblock_changed.adblock_enabled = !base.adblock_enabled;
        assert!(settings_change_effects(&base, &adblock_changed).contains(&SettingsEffect::AdGuardSync));
    }

    fn make_n_test_state(cfg: config::AppConfig, cfg_path: std::path::PathBuf) -> AppState {
        let config_arc = std::sync::Arc::new(tokio::sync::RwLock::new(std::sync::Arc::new(cfg)));
        let ag = std::sync::Arc::new(crate::antigravity::AntigravityManager::new(config_arc.clone()));
        AppState {
            config: config_arc,
            config_path: std::sync::Arc::new(cfg_path),
            http: reqwest::Client::new(),
            failover_log: std::sync::Arc::new(crate::failover::FailoverLog::default()),
            routing_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            config_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            speedtest_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            zapret_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            antigravity: ag,
            vkturn: crate::vkturn::VkTurnManager::new(),
            multiwan: crate::multiwan::MultiWanManager::new(),
        }
    }

    #[tokio::test]
    async fn regression_n01_put_settings_persists_non_routing_changes_to_disk() {
        let dir = std::env::temp_dir().join(format!("xkeen_n01_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cfg_path = dir.join("config.json");
        let yaml_path = dir.join("config.yaml");
        tokio::fs::write(&yaml_path, "proxies: []\nproxy-groups: []\nrules: []\n").await.unwrap();

        let mut cfg = config::AppConfig::default();
        cfg.mihomo.config_path = yaml_path.to_string_lossy().to_string();
        cfg.logs.level = "info".to_string();
        tokio::fs::write(&cfg_path, serde_json::to_string_pretty(&cfg).unwrap()).await.unwrap();

        let state = make_n_test_state(cfg.clone(), cfg_path.clone());
        let mut updated = cfg.clone();
        updated.logs.level = "debug".to_string();

        let resp = put_settings(State(state), Json(serde_json::to_value(&updated).unwrap())).await;
        assert_eq!(resp.status(), axum::http::StatusCode::OK);

        let on_disk: config::AppConfig = serde_json::from_str(
            &tokio::fs::read_to_string(&cfg_path).await.unwrap(),
        )
        .unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(on_disk.logs.level, "debug", "N01: изменение без RoutingYaml должно сохраняться в config.json");
    }

    #[tokio::test]
    async fn regression_n02_put_settings_failover_toggle_does_not_deadlock() {
        let dir = std::env::temp_dir().join(format!("xkeen_n02_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cfg_path = dir.join("config.json");
        let yaml_path = dir.join("config.yaml");
        tokio::fs::write(&yaml_path, "proxies: []\nproxy-groups: []\nrules: []\n").await.unwrap();

        let mut cfg = config::AppConfig::default();
        cfg.mihomo.config_path = yaml_path.to_string_lossy().to_string();
        cfg.failover.enabled = false;
        tokio::fs::write(&cfg_path, serde_json::to_string_pretty(&cfg).unwrap()).await.unwrap();

        let state = make_n_test_state(cfg.clone(), cfg_path.clone());
        let mut updated = cfg.clone();
        updated.failover.enabled = true;

        let res = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            put_settings(State(state), Json(serde_json::to_value(&updated).unwrap())),
        )
        .await;
        let _ = std::fs::remove_dir_all(&dir);
        assert!(res.is_ok(), "N02: переключение failover.enabled не должно вызывать deadlock ConfigTx");
        assert_eq!(res.unwrap().status(), axum::http::StatusCode::OK);
    }

    #[tokio::test]
    async fn regression_n07_save_config_file_rejects_invalid_schema() {
        let dir = std::env::temp_dir().join(format!("xkeen_n07_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cfg_path = dir.join("config.json");

        let mut cfg = config::AppConfig::default();
        cfg.logs.level = "warn".to_string();
        let orig_json = serde_json::to_string_pretty(&cfg).unwrap();
        tokio::fs::write(&cfg_path, &orig_json).await.unwrap();

        let state = make_n_test_state(cfg.clone(), cfg_path.clone());
        let req = SaveConfigFileRequest {
            file: "route".to_string(),
            content: r#"{"mihomo": "broken_schema"}"#.to_string(),
            reload_mihomo: false,
        };

        let resp = save_config_file(State(state.clone()), Json(req)).await;
        let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            body.get("success").and_then(|v| v.as_bool()),
            Some(false),
            "N07: синтаксически корректный JSON с неверной схемой должен отклоняться: {body}"
        );
        let on_disk = tokio::fs::read_to_string(&cfg_path).await.unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(on_disk, orig_json, "N07: config.json не должен перезаписываться при ошибке схемы");
    }

    #[tokio::test]
    async fn regression_r38_put_settings_rejects_invalid_schedules() {
        let dir = std::env::temp_dir().join(format!("xkeen_r38_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cfg_path = dir.join("config.json");
        let cfg = config::AppConfig::default();
        tokio::fs::write(&cfg_path, serde_json::to_string_pretty(&cfg).unwrap()).await.unwrap();

        let state = make_n_test_state(cfg.clone(), cfg_path.clone());
        let mut updated = cfg.clone();
        updated.schedules.push(config::DeviceSchedule {
            id: "bad1".to_string(),
            ip: "192.168.1.50".to_string(),
            days: vec![1],
            time_start: "99:99".to_string(),
            time_end: "23:00".to_string(),
            action: "block".to_string(),
            target_server: None,
            enabled: true,
        });

        let resp = put_settings(State(state), Json(serde_json::to_value(&updated).unwrap())).await;
        let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            body.get("success").and_then(|v| v.as_bool()),
            Some(false),
            "R38: PUT /api/settings должен отклонять невалидные schedules: {body}"
        );
    }

    #[tokio::test]
    async fn regression_reset_settings_restores_defaults() {
        let dir = std::env::temp_dir().join(format!("xkeen_reset_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cfg_path = dir.join("config.json");
        let yaml_path = dir.join("config.yaml");
        tokio::fs::write(&yaml_path, "proxies: []\nproxy-groups: []\nrules: []\n").await.unwrap();

        let mut cfg = config::AppConfig::default();
        cfg.mihomo.config_path = yaml_path.to_string_lossy().to_string();
        cfg.logs.level = "debug".to_string();
        cfg.failover.enabled = true;
        cfg.auth.enabled = true;
        cfg.auth.password_hash = "pbkdf2$test_hash".to_string();
        cfg.auth.session_secret = "test_secret_keep".to_string();
        tokio::fs::write(&cfg_path, serde_json::to_string_pretty(&cfg).unwrap()).await.unwrap();

        let state = make_n_test_state(cfg, cfg_path.clone());
        let resp = reset_settings(State(state.clone())).await;
        assert_eq!(resp.status(), axum::http::StatusCode::OK);

        let on_disk: config::AppConfig = serde_json::from_str(
            &tokio::fs::read_to_string(&cfg_path).await.unwrap(),
        )
        .unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(on_disk.logs.level, "info");
        assert!(!on_disk.failover.enabled);
        assert!(on_disk.auth.enabled, "AUDIT-024: auth.enabled должен сохраняться при сбросе");
        assert_eq!(on_disk.auth.password_hash, "pbkdf2$test_hash", "AUDIT-024: password_hash должен сохраняться");
        assert_eq!(on_disk.auth.session_secret, "test_secret_keep", "AUDIT-024: auth.session_secret должен сохраняться");
    }

    #[tokio::test]
    async fn regression_reset_settings_cleans_free_sub_cache_providers_and_reloads_mihomo() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let dir = std::env::temp_dir().join(format!("xkeen_reset_full_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let prov_dir = dir.join("providers");
        std::fs::create_dir_all(&prov_dir).unwrap();
        let cfg_path = dir.join("config.json");
        let yaml_path = dir.join("config.yaml");
        let cache_path = crate::free_sub::cache_file_path(&cfg_path);

        let initial_yaml = "\
mixed-port: 7892
proxy-providers:
  free_wl:
    type: file
    path: \"./providers/free_wl.yaml\"
  free_vpn:
    type: file
    path: \"./providers/free_vpn.yaml\"
  __free_sub_probe:
    type: file
    path: \"./providers/__free_sub_probe.yaml\"
proxy-groups:
  - name: PROXY
    type: select
    proxies: [DIRECT]
    use:
      - free_wl
      - free_vpn
rules:
  - MATCH,PROXY
";
        tokio::fs::write(&yaml_path, initial_yaml).await.unwrap();
        tokio::fs::write(&prov_dir.join("free_wl.yaml"), "proxies: []\n").await.unwrap();
        tokio::fs::write(&prov_dir.join("free_vpn.yaml"), "proxies: []\n").await.unwrap();
        tokio::fs::write(&prov_dir.join("__free_sub_probe.yaml"), "proxies: []\n").await.unwrap();
        tokio::fs::write(&cache_path, "{}").await.unwrap();

        let reloaded = std::sync::Arc::new(AtomicBool::new(false));
        let switched_direct = std::sync::Arc::new(AtomicBool::new(false));
        let rel_srv = reloaded.clone();
        let dir_srv = switched_direct.clone();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let srv = tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else { break };
                let rel_srv = rel_srv.clone();
                let dir_srv = dir_srv.clone();
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    let first_line = req.lines().next().unwrap_or("");
                    let (status, body) = if first_line.starts_with("GET /version") {
                        ("200 OK", r#"{"version":"v1.19.34"}"#.to_string())
                    } else if first_line.starts_with("GET /proxies") {
                        ("200 OK", serde_json::json!({
                            "proxies": {
                                "PROXY": { "name": "PROXY", "type": "Selector", "now": "some-node", "all": ["DIRECT", "some-node"] },
                                "DIRECT": { "name": "DIRECT", "type": "Direct" }
                            }
                        }).to_string())
                    } else if first_line.starts_with("PUT /configs") {
                        rel_srv.store(true, Ordering::SeqCst);
                        ("204 No Content", String::new())
                    } else if first_line.starts_with("PUT /proxies/PROXY") {
                        if req.contains("\"DIRECT\"") {
                            dir_srv.store(true, Ordering::SeqCst);
                        }
                        ("204 No Content", String::new())
                    } else {
                        ("200 OK", "{}".to_string())
                    };
                    let resp = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(resp.as_bytes()).await;
                    let _ = socket.shutdown().await;
                });
            }
        });

        let mut cfg = config::AppConfig::default();
        cfg.mihomo.host = "127.0.0.1".into();
        cfg.mihomo.port = port;
        cfg.mihomo.config_path = yaml_path.to_string_lossy().to_string();
        cfg.whitelist.enabled = true;
        cfg.whitelist_providers = vec!["free_wl".into()];
        tokio::fs::write(&cfg_path, serde_json::to_string_pretty(&cfg).unwrap()).await.unwrap();

        let state = make_n_test_state(cfg, cfg_path.clone());
        let resp = reset_settings(State(state.clone())).await;
        srv.abort();
        assert_eq!(resp.status(), axum::http::StatusCode::OK);

        assert!(!cache_path.exists(), "free_sub_cache.json должен быть удалён при сбросе");
        assert!(!prov_dir.join("free_wl.yaml").exists(), "free_wl.yaml должен быть удалён");
        assert!(!prov_dir.join("free_vpn.yaml").exists(), "free_vpn.yaml должен быть удалён");
        assert!(!prov_dir.join("__free_sub_probe.yaml").exists(), "__free_sub_probe.yaml должен быть удалён");

        let final_yaml = tokio::fs::read_to_string(&yaml_path).await.unwrap();
        let provs = crate::routing::parse_provider_names(&final_yaml);
        assert!(provs.is_empty(), "провайдеры free_wl/free_vpn/__free_sub_probe должны быть удалены из config.yaml: {provs:?}");
        assert!(reloaded.load(Ordering::SeqCst), "reset_settings должен делать reload Mihomo");
        assert!(switched_direct.load(Ordering::SeqCst), "reset_settings должен возвращать PROXY в DIRECT");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
