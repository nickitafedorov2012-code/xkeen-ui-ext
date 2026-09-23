use axum::extract::{Request, State};
use axum::http::header::{COOKIE, SET_COOKIE};
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Json, Response};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::Path;

use crate::{api::{api_err, api_ok}, config, log_i, log_w, AppState};

pub const COOKIE_NAME: &str = "xr_session";
pub const SESSION_TTL_SECS: u64 = 30 * 24 * 60 * 60; // 30 days

static SEED_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn rand_seed() -> u64 {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let count = SEED_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let state = RandomState::new().build_hasher().finish();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    now.wrapping_add(state).wrapping_add(count)
}

#[derive(Deserialize)]
pub struct LoginRequest {
    pub password: String,
}

#[derive(Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: Option<String>,
    pub new_password: String,
    pub enabled: bool,
}

#[derive(Serialize)]
pub struct AuthStatusResponse {
    pub enabled: bool,
    pub authenticated: bool,
}

/// Генерация псевдослучайной соли (32 hex символа).
pub fn generate_salt() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let pid = std::process::id();
    let mut hasher = Sha256::new();
    hasher.update(format!("xr-salt-{}-{}-{}", now, pid, rand_seed()).as_bytes());
    hex::encode(hasher.finalize())[..32].to_string()
}

/// Генерация секретного ключа сессий.
pub fn generate_secret() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut hasher = Sha256::new();
    hasher.update(format!("xr-secret-{}-{}", now, rand_seed()).as_bytes());
    hex::encode(hasher.finalize())
}

/// Хеширование пароля: SHA256(salt + ":" + password).
pub fn hash_password(password: &str, salt: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!("{}:{}", salt, password).as_bytes());
    hex::encode(hasher.finalize())
}

/// Проверка пароля.
pub fn verify_password(password: &str, salt: &str, expected_hash: &str) -> bool {
    if expected_hash.is_empty() {
        return false;
    }
    let calculated = hash_password(password, salt);
    // Константное по времени сравнение для защиты от timing attacks
    constant_time_eq(&calculated, expected_hash)
}

/// Генерация токена сессии, подписанного секретом.
pub fn create_session_token(secret: &str) -> String {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let payload = format!("{}:{}", ts, rand_seed());
    let mut hasher = Sha256::new();
    hasher.update(format!("{}:{}", secret, payload).as_bytes());
    let sig = hex::encode(hasher.finalize());
    format!("{}.{}", payload, sig)
}

/// Проверка валидности токена сессии (срок жизни — 30 дней).
pub fn verify_session_token(token: &str, secret: &str) -> bool {
    if secret.is_empty() || token.is_empty() {
        return false;
    }
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 2 {
        return false;
    }
    let (payload, sig) = (parts[0], parts[1]);
    let mut hasher = Sha256::new();
    hasher.update(format!("{}:{}", secret, payload).as_bytes());
    let expected_sig = hex::encode(hasher.finalize());
    if !constant_time_eq(sig, &expected_sig) {
        return false;
    }
    // Проверка срока жизни
    if let Some(ts_str) = payload.split(':').next() {
        if let Ok(ts) = ts_str.parse::<u64>() {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            // Токен валиден SESSION_TTL_SECS (30 дней)
            if now >= ts && (now - ts) <= SESSION_TTL_SECS {
                return true;
            }
        }
    }
    false
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.bytes().zip(b.bytes()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Извлечение токена из Cookie или Authorization Bearer заголовка.
pub fn extract_token(req: &Request) -> Option<String> {
    if let Some(auth_hdr) = req.headers().get("authorization") {
        if let Ok(s) = auth_hdr.to_str() {
            if let Some(token) = s.strip_prefix("Bearer ") {
                return Some(token.trim().to_string());
            }
        }
    }
    if let Some(cookie_hdr) = req.headers().get(COOKIE) {
        if let Ok(s) = cookie_hdr.to_str() {
            for item in s.split(';') {
                let trimmed = item.trim();
                if let Some(val) = trimmed.strip_prefix(&format!("{}=", COOKIE_NAME)) {
                    return Some(val.to_string());
                }
            }
        }
    }
    None
}

/// GET /api/auth/status
pub async fn auth_status(State(state): State<AppState>, req: Request) -> Response {
    let cfg = state.config.read().await.clone();
    if !cfg.auth.enabled {
        return api_ok(json!(AuthStatusResponse {
            enabled: false,
            authenticated: true,
        }));
    }
    let authenticated = match extract_token(&req) {
        Some(token) => verify_session_token(&token, &cfg.auth.session_secret),
        None => false,
    };
    api_ok(json!(AuthStatusResponse {
        enabled: true,
        authenticated,
    }))
}

/// POST /api/auth/login
pub async fn login(State(state): State<AppState>, Json(body): Json<LoginRequest>) -> Response {
    let (auth_cfg, config_path) = {
        let cfg = state.config.read().await;
        (cfg.auth.clone(), state.config_path.clone())
    };

    if !auth_cfg.enabled {
        return api_ok(json!({ "authenticated": true, "message": "Авторизация отключена" }));
    }

    if !verify_password(&body.password, &auth_cfg.salt, &auth_cfg.password_hash) {
        log_w!("Неудачная попытка входа в веб-панель (неверный пароль)");
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "success": false, "error": "Неверный пароль" })),
        )
            .into_response();
    }

    // Если секрет пуст, сгенерируем и сохраним
    let secret = if auth_cfg.session_secret.is_empty() {
        let new_secret = generate_secret();
        let _guard = state.config_lock.lock().await;
        let mut cur = (**state.config.read().await).clone();
        cur.auth.session_secret = new_secret.clone();
        let _ = config::save(&config_path, &cur).await;
        *state.config.write().await = std::sync::Arc::new(cur);
        new_secret
    } else {
        auth_cfg.session_secret
    };

    let token = create_session_token(&secret);
    log_i!("Успешный вход в веб-панель");

    let cookie_val = format!(
        "{}={}; Path=/; Max-Age={}; HttpOnly; SameSite=Lax",
        COOKIE_NAME, token, SESSION_TTL_SECS
    );

    let mut response = api_ok(json!({ "authenticated": true, "token": token }));
    if let Ok(hdr) = HeaderValue::from_str(&cookie_val) {
        response.headers_mut().insert(SET_COOKIE, hdr);
    }
    response
}

/// POST /api/auth/logout
pub async fn logout() -> Response {
    let cookie_val = format!("{}=; Path=/; Max-Age=0; HttpOnly; SameSite=Lax", COOKIE_NAME);
    let mut response = api_ok(json!({ "authenticated": false }));
    if let Ok(hdr) = HeaderValue::from_str(&cookie_val) {
        response.headers_mut().insert(SET_COOKIE, hdr);
    }
    response
}

/// POST /api/auth/change-password
pub async fn change_password(
    State(state): State<AppState>,
    Json(body): Json<ChangePasswordRequest>,
) -> Response {
    let _guard = state.config_lock.lock().await;
    let mut cur = (**state.config.read().await).clone();

    // Если пароль уже был включен, требуем подтверждение старого пароля
    if cur.auth.enabled && !cur.auth.password_hash.is_empty() {
        let curr = body.current_password.unwrap_or_default();
        if !verify_password(&curr, &cur.auth.salt, &cur.auth.password_hash) {
            return api_err("Текущий пароль указан неверно");
        }
    }

    if body.enabled {
        if body.new_password.trim().is_empty() && cur.auth.password_hash.is_empty() {
            return api_err("Пароль не может быть пустым");
        }
        if !body.new_password.trim().is_empty() {
            let salt = generate_salt();
            let hash = hash_password(&body.new_password, &salt);
            cur.auth.salt = salt;
            cur.auth.password_hash = hash;
            cur.auth.session_secret = generate_secret();
        }
        cur.auth.enabled = true;
        log_i!("Включена защита паролем веб-панели");
    } else {
        cur.auth.enabled = false;
        log_i!("Защита паролем веб-панели отключена");
    }

    if let Err(e) = config::save(&state.config_path, &cur).await {
        return api_err(format!("Ошибка сохранения настроек: {}", e));
    }
    *state.config.write().await = std::sync::Arc::new(cur);
    api_ok(json!({ "saved": true, "enabled": body.enabled }))
}

/// Middleware защиты API
pub async fn auth_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();

    // Разрешаем статику фронтенда и незащищенные эндпоинты
    if !path.starts_with("/api/")
        || path == "/api/auth/login"
        || path == "/api/auth/status"
        || path == "/patch"
        || path == "/api/antigravity/fix.cmd"
    {
        return next.run(req).await;
    }

    let cfg = state.config.read().await.clone();
    if !cfg.auth.enabled {
        return next.run(req).await;
    }

    let is_valid = match extract_token(&req) {
        Some(token) => verify_session_token(&token, &cfg.auth.session_secret),
        None => false,
    };

    if is_valid {
        next.run(req).await
    } else {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "success": false,
                "error": "unauthorized",
                "auth_required": true,
                "message": "Требуется авторизация"
            })),
        )
            .into_response()
    }
}

fn restart_service_if_exists() {
    let init_path = Path::new("/opt/etc/init.d/S99xkeen-route");
    if init_path.exists() {
        println!("[INFO] Перезапуск сервиса xkeen-route для применения настроек...");
        let _ = std::process::Command::new(init_path).arg("restart").status();
    }
}

/// CLI сброс пароля (xkeen-route reset-password)
pub fn cli_reset_password(config_path: &Path) -> std::io::Result<()> {
    let mut cfg = config::load(config_path);
    cfg.auth.enabled = false;
    cfg.auth.password_hash.clear();
    cfg.auth.salt.clear();
    cfg.auth.session_secret.clear();
    let serialized = serde_json::to_string_pretty(&cfg).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    std::fs::write(config_path, serialized)?;
    println!("[OK] Пароль успешно сброшен. Авторизация отключена.");
    restart_service_if_exists();
    Ok(())
}

/// CLI установка пароля (xkeen-route set-password <pass>)
pub fn cli_set_password(config_path: &Path, password: &str) -> std::io::Result<()> {
    if password.trim().is_empty() {
        eprintln!("[ERROR] Пароль не может быть пустым");
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Empty password"));
    }
    let mut cfg = config::load(config_path);
    let salt = generate_salt();
    let hash = hash_password(password, &salt);
    cfg.auth.enabled = true;
    cfg.auth.salt = salt;
    cfg.auth.password_hash = hash;
    cfg.auth.session_secret = generate_secret();
    let serialized = serde_json::to_string_pretty(&cfg).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    std::fs::write(config_path, serialized)?;
    println!("[OK] Новый пароль установлен. Авторизация включена.");
    restart_service_if_exists();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_hash_verify() {
        let salt = generate_salt();
        let pass = "keenetic123";
        let hash = hash_password(pass, &salt);
        assert!(verify_password(pass, &salt, &hash));
        assert!(!verify_password("wrong_pass", &salt, &hash));
    }

    #[test]
    fn session_token_roundtrip() {
        let secret = generate_secret();
        let token = create_session_token(&secret);
        assert!(verify_session_token(&token, &secret));
        assert!(!verify_session_token(&token, "wrong_secret"));
        assert!(!verify_session_token("invalid.token", &secret));
    }
}
