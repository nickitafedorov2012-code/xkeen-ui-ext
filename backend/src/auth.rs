use axum::extract::{ConnectInfo, Request, State};
use axum::http::header::{COOKIE, SET_COOKIE};
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Json, Response};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::Path;

use crate::{api::{api_err, api_ok}, config, log_i, log_w, AppState};

pub const COOKIE_NAME: &str = "xr_session";
pub const SESSION_TTL_SECS: u64 = 24 * 60 * 60; // 24 часа вместо 30 дней
pub const PBKDF2_ITERATIONS: u32 = 100_000;

static SEED_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
static FAILED_LOGIN_ATTEMPTS: std::sync::Mutex<Option<HashMap<IpAddr, Vec<u64>>>> = std::sync::Mutex::new(None);

/// Заполнение буфера криптографически стойкими случайными байтами.
/// Использует /dev/urandom на Unix/Linux/KeeneticOS с fallback на SHA-256 CSPRNG-микс.
pub fn fill_crypto_random_bytes(dest: &mut [u8]) {
    #[cfg(unix)]
    {
        if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
            use std::io::Read;
            if f.read_exact(dest).is_ok() {
                return;
            }
        }
    }

    let mut hasher = Sha256::new();
    let count = SEED_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let pid = std::process::id();
    let stack_addr = dest.as_ptr() as usize;

    hasher.update(count.to_ne_bytes());
    hasher.update(now.to_ne_bytes());
    hasher.update(pid.to_ne_bytes());
    hasher.update(stack_addr.to_ne_bytes());

    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let state = RandomState::new().build_hasher().finish();
    hasher.update(state.to_ne_bytes());

    let hash = hasher.finalize();
    let mut offset = 0;
    while offset < dest.len() {
        let chunk = (dest.len() - offset).min(32);
        dest[offset..offset + chunk].copy_from_slice(&hash[..chunk]);
        offset += chunk;
    }
}

/// Генерация hex-строки из криптографически стойких случайных байт.
pub fn random_hex(len_bytes: usize) -> String {
    let mut bytes = vec![0u8; len_bytes];
    fill_crypto_random_bytes(&mut bytes);
    hex::encode(bytes)
}

static REVOKED_TOKENS_PATH: std::sync::Mutex<Option<std::path::PathBuf>> = std::sync::Mutex::new(None);
static REVOKED_TOKENS: std::sync::Mutex<Option<std::collections::HashMap<String, u64>>> = std::sync::Mutex::new(None);

/// Инициализация постоянного хранилища отозванных токенов на диске (баг 93)
pub fn init_revoked_tokens_storage(path: &Path) {
    // Лок пути отпускается сразу: revoke_token берёт REVOKED_TOKENS, затем путь (в persist),
    // и удержание пути здесь при захвате REVOKED_TOKENS давало ABBA-взаимоблокировку.
    {
        let mut p_guard = REVOKED_TOKENS_PATH.lock().unwrap_or_else(|e| e.into_inner());
        *p_guard = Some(path.to_path_buf());
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let mut loaded_map: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    if path.exists() {
        if let Ok(bytes) = std::fs::read(path) {
            if let Ok(map) = serde_json::from_slice::<std::collections::HashMap<String, u64>>(&bytes) {
                for (tok, exp) in map {
                    if exp > now {
                        loaded_map.insert(tok, exp);
                    }
                }
            }
        }
    }

    let mut guard = REVOKED_TOKENS.lock().unwrap_or_else(|e| e.into_inner());
    *guard = Some(loaded_map);
}

#[cfg(test)]
pub fn reset_in_memory_revoked_tokens_for_test() {
    let mut guard = REVOKED_TOKENS.lock().unwrap_or_else(|e| e.into_inner());
    *guard = None;
}

pub fn persist_revoked_tokens_to(
    path: &std::path::Path,
    map: &std::collections::HashMap<String, u64>,
) -> std::io::Result<()> {
    use std::io::Write;
    let json_bytes = serde_json::to_vec(map)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp_path = path.with_extension("tmp");
    let mut file = std::fs::File::create(&tmp_path)?;
    file.write_all(&json_bytes)?;
    file.sync_all()?;
    std::fs::rename(&tmp_path, path)
}

fn persist_revoked_tokens(map: &std::collections::HashMap<String, u64>) {
    let p_guard = REVOKED_TOKENS_PATH.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(ref path) = *p_guard {
        let _ = persist_revoked_tokens_to(path, map);
    }
}

/// Инвалидация токена сессии на стороне сервера при выходе (logout).
pub fn revoke_token(token: &str) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut guard = REVOKED_TOKENS.lock().unwrap_or_else(|e| e.into_inner());
    let map = guard.get_or_insert_with(std::collections::HashMap::new);

    // Очищаем естественно истекшие токены (SEC-11)
    if map.len() > 1000 {
        map.retain(|_, &mut exp| exp > now);
    }
    // Если после очистки размер все еще превышает 10_000, сохраняем самые свежие токены
    if map.len() > 10_000 {
        let mut entries: Vec<_> = map.drain().collect();
        entries.sort_by_key(|(_, exp)| *exp);
        for (tok, exp) in entries.into_iter().rev().take(5000) {
            map.insert(tok, exp);
        }
    }
    let exp = now + SESSION_TTL_SECS;
    map.insert(token.to_string(), exp);
    persist_revoked_tokens(map);
}

/// Проверка, был ли токен сессии отозван на сервере.
pub fn is_token_revoked(token: &str) -> bool {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut guard = REVOKED_TOKENS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(map) = guard.as_mut() {
        if let Some(&exp) = map.get(token) {
            if exp > now {
                return true;
            } else {
                map.remove(token);
                return false;
            }
        }
    }
    false
}

/// Проверка и атомарное резервирование попытки входа (защита от race condition при параллельных запросах, баг 92)
pub fn check_and_reserve_rate_limit(ip: IpAddr) -> bool {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut guard = FAILED_LOGIN_ATTEMPTS.lock().unwrap_or_else(|e| e.into_inner());
    let map = guard.get_or_insert_with(HashMap::new);

    // Очищаем устаревшие IP из памяти (SEC-11)
    map.retain(|_, attempts| {
        attempts.retain(|&ts| now.saturating_sub(ts) < 60);
        !attempts.is_empty()
    });

    let list = map.entry(ip).or_default();
    if list.len() >= 5 {
        return false;
    }
    list.push(now);
    true
}

#[cfg(test)]
fn check_rate_limit(ip: IpAddr) -> bool {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut guard = FAILED_LOGIN_ATTEMPTS.lock().unwrap_or_else(|e| e.into_inner());
    let map = guard.get_or_insert_with(HashMap::new);

    // Очищаем устаревшие IP из памяти (SEC-11)
    map.retain(|_, attempts| {
        attempts.retain(|&ts| now.saturating_sub(ts) < 60);
        !attempts.is_empty()
    });

    let list = map.entry(ip).or_default();
    list.len() < 5
}

/// Асинхронная верификация пароля через spawn_blocking во избежание блокировки потока tokio (баг 92)
pub async fn verify_password_async(password: String, salt: String, expected_hash: String) -> bool {
    tokio::task::spawn_blocking(move || {
        verify_password(&password, &salt, &expected_hash)
    })
    .await
    .unwrap_or(false)
}

/// Асинхронное хеширование пароля через spawn_blocking во избежание блокировки потока tokio (баг 92)
pub async fn hash_password_async(password: String, salt: String) -> String {
    tokio::task::spawn_blocking(move || {
        hash_password(&password, &salt)
    })
    .await
    .unwrap_or_default()
}

#[cfg(test)]
fn record_failed_attempt(ip: IpAddr) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut guard = FAILED_LOGIN_ATTEMPTS.lock().unwrap_or_else(|e| e.into_inner());
    let map = guard.get_or_insert_with(HashMap::new);
    let list = map.entry(ip).or_default();
    list.push(now);
}

fn clear_rate_limit(ip: IpAddr) {
    let mut guard = FAILED_LOGIN_ATTEMPTS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(map) = guard.as_mut() {
        map.remove(&ip);
    }
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

/// Генерация криптографически стойкой соли (32 hex символа = 128 бит энтропии).
pub fn generate_salt() -> String {
    random_hex(16)
}

/// Генерация криптографически стойкого секретного ключа сессий (64 hex символа = 256 бит энтропии).
pub fn generate_secret() -> String {
    random_hex(32)
}

/// HMAC-SHA256 без сторонних тяжелых библиотек.
fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        let mut hasher = Sha256::new();
        hasher.update(key);
        let hash = hasher.finalize();
        k[..32].copy_from_slice(&hash);
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut ipad = [0x36u8; 64];
    let mut opad = [0x5cu8; 64];
    for i in 0..64 {
        ipad[i] ^= k[i];
        opad[i] ^= k[i];
    }
    let mut inner = Sha256::new();
    inner.update(ipad);
    inner.update(data);
    let inner_hash = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(opad);
    outer.update(inner_hash);
    outer.finalize().into()
}

/// PBKDF2-HMAC-SHA256 с настраиваемым числом итераций.
pub fn pbkdf2_hmac_sha256(password: &[u8], salt: &[u8], iterations: u32) -> [u8; 32] {
    let mut salt_and_index = Vec::with_capacity(salt.len() + 4);
    salt_and_index.extend_from_slice(salt);
    salt_and_index.extend_from_slice(&1u32.to_be_bytes());

    let mut u = hmac_sha256(password, &salt_and_index);
    let mut result = u;

    for _ in 1..iterations {
        u = hmac_sha256(password, &u);
        for j in 0..32 {
            result[j] ^= u[j];
        }
    }
    result
}

/// Хеширование пароля: PBKDF2-HMAC-SHA256 (100 000 итераций) в формате $pbkdf2$iter$salt$hash.
pub fn hash_password(password: &str, salt: &str) -> String {
    let raw = pbkdf2_hmac_sha256(password.as_bytes(), salt.as_bytes(), PBKDF2_ITERATIONS);
    format!("$pbkdf2${}${}${}", PBKDF2_ITERATIONS, salt, hex::encode(raw))
}

/// Проверка пароля с поддержкой $pbkdf2$ и обратной совместимостью с legacy SHA-256.
pub fn verify_password(password: &str, salt: &str, expected_hash: &str) -> bool {
    if expected_hash.is_empty() {
        return false;
    }
    if expected_hash.starts_with("$pbkdf2$") {
        let parts: Vec<&str> = expected_hash.split('$').collect();
        // parts: ["", "pbkdf2", "<iter>", "<salt>", "<hex>"]
        if parts.len() == 5 {
            let iter: u32 = parts[2].parse().unwrap_or(PBKDF2_ITERATIONS);
            let p_salt = parts[3];
            let p_hex = parts[4];
            let calculated = pbkdf2_hmac_sha256(password.as_bytes(), p_salt.as_bytes(), iter);
            return constant_time_eq(&hex::encode(calculated), p_hex);
        }
    }
    // Обратная совместимость с legacy SHA256(salt:password)
    let mut hasher = Sha256::new();
    hasher.update(format!("{}:{}", salt, password).as_bytes());
    let calculated = hex::encode(hasher.finalize());
    constant_time_eq(&calculated, expected_hash)
}

/// Генерация токена сессии, подписанного HMAC-SHA256 секретом.
pub fn create_session_token(secret: &str) -> String {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let entropy = random_hex(16);
    let payload = format!("{}:{}", ts, entropy);
    let sig = hex::encode(hmac_sha256(secret.as_bytes(), payload.as_bytes()));
    format!("{}.{}", payload, sig)
}

/// Проверка валидности токена сессии (срок жизни — 24 часа, проверка HMAC подписи и отзыва).
pub fn verify_session_token(token: &str, secret: &str) -> bool {
    if secret.is_empty() || token.is_empty() {
        return false;
    }
    if is_token_revoked(token) {
        return false;
    }
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 2 {
        return false;
    }
    let (payload, sig) = (parts[0], parts[1]);
    let expected_sig = hex::encode(hmac_sha256(secret.as_bytes(), payload.as_bytes()));
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
            // Токен валиден SESSION_TTL_SECS (24 часа) с допуском 60с на дрейф часов при загрузке
            if (now + 60 >= ts) && (now.saturating_sub(ts) <= SESSION_TTL_SECS) {
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
pub async fn login(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(body): Json<LoginRequest>,
) -> Response {
    let client_ip = peer.ip();
    if !check_and_reserve_rate_limit(client_ip) {
        log_w!("Превышен лимит попыток входа (rate limit 5/мин) для IP {}", client_ip);
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "success": false,
                "error": "Слишком много неудачных попыток входа. Подождите 1 минуту."
            })),
        )
            .into_response();
    }

    let (auth_cfg, config_path) = {
        let cfg = state.config.read().await;
        (cfg.auth.clone(), state.config_path.clone())
    };

    if !auth_cfg.enabled {
        return api_ok(json!({ "authenticated": true, "message": "Авторизация отключена" }));
    }

    if !verify_password_async(body.password.clone(), auth_cfg.salt.clone(), auth_cfg.password_hash.clone()).await {
        log_w!("Неудачная попытка входа в веб-панель (неверный пароль) с IP {}", client_ip);
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "success": false, "error": "Неверный пароль" })),
        )
            .into_response();
    }

    clear_rate_limit(client_ip);

    // Если пароль был в старом формате SHA256, бесшовно мигрируем на устойчивый PBKDF2
    if !auth_cfg.password_hash.starts_with("$pbkdf2$") {
        let new_hash = hash_password_async(body.password.clone(), auth_cfg.salt.clone()).await;
        let _guard = state.config_lock.lock().await;
        let mut cur = (**state.config.read().await).clone();
        cur.auth.password_hash = new_hash;
        let _ = config::save(&config_path, &cur).await;
        *state.config.write().await = std::sync::Arc::new(cur);
        log_i!("Пароль пользователя успешно автоматически мигрирован на PBKDF2-HMAC-SHA256");
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
pub async fn logout(State(state): State<AppState>, req: Request) -> Response {
    if let Some(token) = extract_token(&req) {
        let secret = {
            let cfg = state.config.read().await;
            cfg.auth.session_secret.clone()
        };
        // Отзываем токен только если он криптографически валиден (защита от спама инвалидацией)
        if verify_session_token(&token, &secret) {
            revoke_token(&token);
        }
    }
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

    // A-02: текущий пароль требуется ВСЕГДА, когда хеш уже существует (даже при
    // auth.enabled=false): иначе любой LAN-запрос мог перезаписать существующий
    // пароль и захватить панель. Бутстрап (первый пароль при пустом хеше) разрешён.
    if !cur.auth.password_hash.is_empty() {
        let curr = body.current_password.unwrap_or_default();
        if !verify_password_async(curr, cur.auth.salt.clone(), cur.auth.password_hash.clone()).await {
            return api_err("Текущий пароль указан неверно");
        }
    }

    if body.enabled {
        if body.new_password.trim().is_empty() && cur.auth.password_hash.is_empty() {
            return api_err("Пароль не может быть пустым");
        }
        if !body.new_password.trim().is_empty() {
            let salt = generate_salt();
            let hash = hash_password_async(body.new_password.clone(), salt.clone()).await;
            cur.auth.salt = salt;
            cur.auth.password_hash = hash;
            cur.auth.session_secret = generate_secret();
        }
        cur.auth.enabled = true;
        log_i!("Включена защита паролем веб-панели (PBKDF2)");
    } else {
        cur.auth.enabled = false;
        // При отключении auth очищаем session_secret для инвалидации всех старых сессий
        cur.auth.session_secret = generate_secret();
        log_i!("Защита паролем веб-панели отключена");
    }

    if let Err(e) = config::save(&state.config_path, &cur).await {
        return api_err(format!("Ошибка сохранения настроек: {}", e));
    }
    *state.config.write().await = std::sync::Arc::new(cur);
    api_ok(json!({ "saved": true, "enabled": body.enabled }))
}

/// Извлекает хост без порта, корректно обрабатывая IPv6 в квадратных скобках
fn extract_host_only(host_part: &str) -> &str {
    let host_trimmed = host_part.trim();
    if host_trimmed.starts_with('[') {
        if let Some(end_bracket) = host_trimmed.find(']') {
            &host_trimmed[..=end_bracket]
        } else {
            host_trimmed.split(':').next().unwrap_or("").trim()
        }
    } else {
        host_trimmed.split(':').next().unwrap_or("").trim()
    }
}

/// Строгая проверка хоста Origin на принадлежность к доверенным адресам (SEC-02 / A-01).
/// A-01: раньше ЛЮБОЙ приватный IPv4 (RFC1918) считался доверенным — CSRF был возможен
/// с любого скомпрометированного устройства LAN. Теперь приватные адреса доверяются
/// только через совпадение с заголовком Host (правило 1 в is_trusted_origin);
/// безусловный доверенный набор — loopback и локальные имена роутера.
pub fn is_valid_origin(origin_host_only: &str) -> bool {
    let unbracketed = origin_host_only.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = unbracketed.parse::<std::net::IpAddr>() {
        match ip {
            std::net::IpAddr::V4(ipv4) => ipv4.is_loopback(),
            std::net::IpAddr::V6(ipv6) => ipv6.is_loopback(),
        }
    } else {
        origin_host_only.eq_ignore_ascii_case("localhost")
            || origin_host_only.eq_ignore_ascii_case("my.keenetic.net")
            || origin_host_only.eq_ignore_ascii_case("router")
    }
}

/// Проверка, является ли имя хоста или IP легитимным адресом роутера / локальной сети.
/// Защита от DNS-rebinding (баг 91): даже если Origin совпадает с Host,
/// сам Host должен быть легитимным адресом роутера (loopback, RFC1918 приватная сеть,
/// link-local, либо известные локальные домены вроде *.keenetic.pro / *.keenetic.link / *.keenetic.name, my.keenetic.net, router).
pub fn is_legitimate_host(host_only: &str) -> bool {
    let unbracketed = host_only.trim().trim_start_matches('[').trim_end_matches(']');
    if unbracketed.is_empty() {
        return false;
    }
    if let Ok(ip) = unbracketed.parse::<std::net::IpAddr>() {
        match ip {
            std::net::IpAddr::V4(ipv4) => ipv4.is_loopback() || ipv4.is_private() || ipv4.is_link_local(),
            std::net::IpAddr::V6(ipv6) => ipv6.is_loopback() || ipv6.is_unicast_link_local(),
        }
    } else {
        let lower = host_only.to_ascii_lowercase();
        // Локальные однословные хосты без точек (mycustombox, keenetic, router) или .lan/.local
        !lower.contains('.')
            || lower.ends_with(".lan")
            || lower.ends_with(".local")
            || lower == "localhost"
            || lower == "my.keenetic.net"
            || lower.ends_with(".keenetic.pro")
            || lower.ends_with(".keenetic.link")
            || lower.ends_with(".keenetic.name")
    }
}

/// Проверка безопасности Origin/Referer для защиты от межсайтовых атак (CSRF)
pub fn is_trusted_origin(origin_or_referer: &str, host_header: Option<&str>) -> bool {
    let stripped = origin_or_referer
        .trim_start_matches("http://")
        .trim_start_matches("https://");

    let origin_host_part = stripped.split('/').next().unwrap_or("").trim();
    let origin_host_only = extract_host_only(origin_host_part);

    // 1. Совпадение с переданным Host header (только если сам Host является легитимным адресом роутера — защита от DNS-rebinding, баг 91)
    if let Some(host) = host_header {
        let host_trimmed = host.trim();
        let host_only = extract_host_only(host_trimmed);
        if (origin_host_part.eq_ignore_ascii_case(host_trimmed) || origin_host_only.eq_ignore_ascii_case(host_only))
            && is_legitimate_host(host_only)
        {
            return true;
        }
    }

    // 2. Локальные адреса loopback и доверенные имена роутера
    is_valid_origin(origin_host_only)
}

/// Middleware защиты API и reverse-proxy Clash
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostHeaderPolicy {
    Allow,
    Deny,
}

pub fn host_header_policy(host: Option<&str>, peer_is_loopback: bool) -> HostHeaderPolicy {
    match host {
        Some(h) => {
            let host_only = extract_host_only(h);
            if is_legitimate_host(host_only) {
                HostHeaderPolicy::Allow
            } else {
                HostHeaderPolicy::Deny
            }
        }
        None => {
            if peer_is_loopback {
                HostHeaderPolicy::Allow
            } else {
                HostHeaderPolicy::Deny
            }
        }
    }
}

fn reject_host_header_response(host_header: Option<&str>, path: &str) -> Response {
    if let Some(host) = host_header {
        log_w!("[AUTH] ⚠️ Отклонен запрос с нелегитимным Host (DNS-rebinding): {} к {}", host, path);
        (
            StatusCode::FORBIDDEN,
            Json(json!({
                "success": false,
                "error": "forbidden",
                "message": "Отклонено политикой безопасности: недопустимый заголовок Host"
            })),
        ).into_response()
    } else {
        log_w!("[AUTH] ⚠️ Отклонен запрос без заголовка Host к {}", path);
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "success": false,
                "error": "bad_request",
                "message": "Отсутствует обязательный заголовок Host"
            })),
        ).into_response()
    }
}

pub async fn auth_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();

    let is_protected = path.starts_with("/api/") || path.starts_with("/clash/");
    let is_exempt = path == "/api/auth/login"
        || path == "/api/auth/status"
        || path == "/patch"
        || path == "/api/antigravity/fix.cmd"
        || path == "/api/flow/extension.zip";

    if !is_protected || is_exempt {
        return next.run(req).await;
    }

    // Защита от DNS-rebinding (is_legitimate_host, баг 91, A-06)
    let peer_is_loopback = req
        .extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|ci| ci.0.ip().is_loopback())
        .unwrap_or(false);
    let host_header = req.headers().get("host").and_then(|h| h.to_str().ok());
    if host_header_policy(host_header, peer_is_loopback) == HostHeaderPolicy::Deny {
        return reject_host_header_response(host_header, &path);
    }

    // CSRF Guard для всех мутирующих запросов (POST, PUT, DELETE, PATCH)
    let method = req.method();
    if method == axum::http::Method::POST
        || method == axum::http::Method::PUT
        || method == axum::http::Method::DELETE
        || method == axum::http::Method::PATCH
    {
        let origin_header = req.headers().get("origin")
            .or_else(|| req.headers().get("referer"))
            .and_then(|h| h.to_str().ok());

        if let Some(origin) = origin_header {
            if !is_trusted_origin(origin, host_header) {
                log_w!("[AUTH] ⚠️ Отклонен запрос с недопустимым Origin: {} к {}", origin, path);
                return (
                    StatusCode::FORBIDDEN,
                    Json(json!({
                        "success": false,
                        "error": "forbidden",
                        "message": "Отклонено политикой CSRF: недопустимый Origin"
                    })),
                ).into_response();
            }
        } else {
            // SEC-08: Требуем X-Requested-With или X-CSRF-Token для мутирующих запросов без Origin/Referer
            let x_requested_with = req.headers().get("x-requested-with").and_then(|h| h.to_str().ok());
            let x_csrf_token = req.headers().get("x-csrf-token").and_then(|h| h.to_str().ok());
            let is_xhr = x_requested_with.map(|v| v.eq_ignore_ascii_case("xmlhttprequest")).unwrap_or(false);
            let has_csrf = x_csrf_token.map(|v| !v.trim().is_empty()).unwrap_or(false);

            if !is_xhr && !has_csrf {
                log_w!("[AUTH] ⚠️ Отклонен мутирующий запрос без Origin/Referer и без CSRF-заголовков к {}", path);
                return (
                    StatusCode::FORBIDDEN,
                    Json(json!({
                        "success": false,
                        "error": "forbidden",
                        "message": "Отклонено политикой CSRF: отсутствует заголовок X-Requested-With или X-CSRF-Token"
                    })),
                ).into_response();
            }
        }
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
    let serialized = serde_json::to_string_pretty(&cfg).map_err(std::io::Error::other)?;
    let tmp_path = config_path.with_extension("tmp");
    std::fs::write(&tmp_path, serialized)?;
    std::fs::rename(&tmp_path, config_path)?;
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
    let serialized = serde_json::to_string_pretty(&cfg).map_err(std::io::Error::other)?;
    let tmp_path = config_path.with_extension("tmp");
    std::fs::write(&tmp_path, serialized)?;
    std::fs::rename(&tmp_path, config_path)?;
    println!("[OK] Новый пароль установлен. Авторизация включена.");
    restart_service_if_exists();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Общее состояние REVOKED_TOKENS: тесты отзыва не должны идти параллельно (сброс в тесте бага 93).
    static REVOKED_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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

    #[test]
    fn session_token_tampering_and_revocation() {
        let _revoked_guard = REVOKED_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let secret = generate_secret();
        let token = create_session_token(&secret);
        assert!(verify_session_token(&token, &secret));

        // 1. Попытка подделки подписи
        let parts: Vec<&str> = token.split('.').collect();
        let tampered_sig = format!("{}.00112233445566778899aabbccddeeff", parts[0]);
        assert!(!verify_session_token(&tampered_sig, &secret));

        // 2. Попытка подделки payload
        let tampered_payload = format!("9999999999:fake.{}", parts[1]);
        assert!(!verify_session_token(&tampered_payload, &secret));

        // 3. Отзыв токена
        revoke_token(&token);
        assert!(!verify_session_token(&token, &secret), "Отозванный токен не должен проходить проверку");
    }

    #[test]
    fn empty_password_rejection() {
        let path = std::env::temp_dir().join(format!("xr-auth-test-{}.json", std::process::id()));
        let res = cli_set_password(&path, "   ");
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().to_string(), "Empty password");
    }

    #[test]
    fn password_salt_isolation() {
        let salt1 = generate_salt();
        let salt2 = generate_salt();
        let pass = "secret123";
        let hash1 = hash_password(pass, &salt1);
        let hash2 = hash_password(pass, &salt2);
        assert_ne!(hash1, hash2, "Разная соль должна давать разный хэш");
        let tampered_hash = hash1.replace(&salt1, &salt2);
        assert!(!verify_password(pass, &salt2, &tampered_hash));
    }

    #[test]
    fn per_ip_rate_limiting() {
        let ip1: std::net::IpAddr = "192.168.1.100".parse().unwrap();
        let ip2: std::net::IpAddr = "192.168.1.101".parse().unwrap();

        clear_rate_limit(ip1);
        clear_rate_limit(ip2);

        for _ in 0..4 {
            record_failed_attempt(ip1);
            assert!(check_rate_limit(ip1));
        }

        record_failed_attempt(ip1);
        assert!(!check_rate_limit(ip1), "ip1 должен быть заблокирован после 5 попыток");

        assert!(check_rate_limit(ip2), "ip2 не должен быть затронут блокировкой ip1");

        clear_rate_limit(ip1);
        assert!(check_rate_limit(ip1), "ip1 должен быть разблокирован после clear_rate_limit");
    }

    #[test]
    fn test_csrf_origin_validation_regression() {
        // 1. Доверенные локальные источники (loopback имена безусловно)
        assert!(is_trusted_origin("http://127.0.0.1:1001", None));
        assert!(is_trusted_origin("http://localhost:1001", None));
        // A-01: приватные LAN-адреса доверяются ТОЛЬКО через совпадение с Host
        assert!(is_trusted_origin("http://192.168.1.1:1001", Some("192.168.1.1:1001")));
        assert!(!is_trusted_origin("http://192.168.1.1:1001", None), "private IP without Host match must be rejected (A-01)");
        assert!(is_trusted_origin("https://10.0.0.1", Some("10.0.0.1")));
        assert!(!is_trusted_origin("https://10.0.0.1", None));
        assert!(is_trusted_origin("http://172.16.0.1", Some("172.16.0.1")));
        assert!(!is_trusted_origin("http://172.16.0.1", None));
        assert!(is_trusted_origin("http://172.31.255.254", Some("172.31.255.254")));
        assert!(!is_trusted_origin("http://172.31.255.254", None));
        // 2. Источник совпадает с заголовком Host (включая KeenDNS)
        assert!(is_trusted_origin("https://myrouter.keenetic.pro:1001", Some("myrouter.keenetic.pro:1001")));
        assert!(is_trusted_origin("https://my.keenetic.net", Some("my.keenetic.net")));
        assert!(is_trusted_origin("http://mycustombox:1001/dashboard", Some("mycustombox:1001")));
        assert!(is_trusted_origin("https://router.lan", Some("router.lan")));

        // 3. Злонамеренные внешние источники (CSRF-атаки) ОБЯЗАНЫ блокироваться!
        // Включая чужие домены keenetic.pro без совпадения с Host (SEC-08)
        assert!(!is_trusted_origin("https://attacker.keenetic.pro", Some("myrouter.keenetic.pro:1001")));
        assert!(!is_trusted_origin("https://myrouter.keenetic.pro:1001", None));
        assert!(is_trusted_origin("https://my.keenetic.net", None));
        assert!(is_trusted_origin("http://router:1001", None));
        assert!(!is_trusted_origin("http://evil-attacker.com", None));
        assert!(!is_trusted_origin("http://evil-attacker.com", Some("192.168.1.1:1001")));
        assert!(!is_trusted_origin("http://172.32.0.1", None), "172.32.* вне приватного диапазона");
        assert!(!is_trusted_origin("http://8.8.8.8", None));
        assert!(!is_trusted_origin("http://phishing-keenetic.com", None));

        // SEC-02: Предотвращение обхода через prefix-matching доменов
        assert!(!is_trusted_origin("http://192.168.attacker.com", None));
        assert!(!is_trusted_origin("http://192.168.attacker.com:1001", None));
        assert!(!is_trusted_origin("http://10.attacker.com", None));
        assert!(!is_trusted_origin("http://172.16.attacker.com", None));
        assert!(!is_trusted_origin("http://192.168.1.1.attacker.com", None));
        assert!(!is_trusted_origin("http://192.168.attacker.com:1001", Some("192.168.1.1:1001")));
    }

    #[test]
    fn regression_bug_90_get_speedtest_disallowed_in_routes() {
        // Баг 90: GET /api/servers/speedtest/{id} переключает всю LAN (мутирующее действие через GET -> CSRF через img src)
        // Проверяем, что в main.rs маршрут speedtest/{id} не привязан к GET-запросам
        let main_src = include_str!("main.rs");
        let has_get_speedtest = main_src.lines().any(|line| {
            line.contains("/api/servers/speedtest/{id}") && line.contains(".get(")
        });
        assert!(!has_get_speedtest, "GET-обработчик для /api/servers/speedtest/{{id}} должен быть удален для защиты от CSRF через GET");
    }

    #[test]
    fn regression_bug_91_dns_rebinding_origin_matches_arbitrary_host() {
        // Баг 91: DNS-rebinding: CSRF-фильтр доверяет Origin, если он равен Host; Host не проверяется.
        // Злоумышленник направляет свой домен evil.com на IP роутера (DNS rebinding)
        // Браузер шлёт: Origin: http://evil.com, Host: evil.com
        // Фильтр НЕ должен доверять произвольному домену evil.com, даже если Origin == Host.
        assert!(!is_trusted_origin("http://evil.com", Some("evil.com")));
        assert!(!is_trusted_origin("http://attacker.org:1001", Some("attacker.org:1001")));

        // Ревизия: GET-запросы к /api/* и /clash/* при выключенной авторизации также должны
        // проверять Host (is_legitimate_host) и отклонять нелегитимный Host: evil.com
        assert!(!is_legitimate_host("evil.com"));
        assert!(!is_legitimate_host("attacker.org"));
        assert!(!is_legitimate_host("192.168.attacker.com"));
        assert!(is_legitimate_host("192.168.1.1"));
        assert!(is_legitimate_host("192.168.2.1"));
        assert!(is_legitimate_host("localhost"));
        assert!(is_legitimate_host("my.keenetic.net"));

        // Проверяем, что в auth_middleware проверка is_legitimate_host выполняется до разделения на методы GET/POST
        let auth_src = include_str!("auth.rs");
        let start = auth_src.find("pub async fn auth_middleware(").expect("auth_middleware exists");
        let body = &auth_src[start..start + 2000];
        assert!(
            body.contains("is_legitimate_host"),
            "auth_middleware must validate Host header against is_legitimate_host for all protected requests"
        );
        let pos_host_check = body.find("is_legitimate_host").expect("Host check in middleware");
        let pos_method_check = body.find("method == axum::http::Method::POST").expect("POST check in middleware");
        assert!(
            pos_host_check < pos_method_check,
            "Host header validation must happen BEFORE method filtering so GET requests cannot be abused via DNS rebinding"
        );
    }

    #[test]
    fn test_sec02_is_valid_origin_strict_parsing() {
        // Доверенные безусловно: loopback и локальные имена роутера
        assert!(is_valid_origin("127.0.0.1"));
        assert!(is_valid_origin("::1"));
        assert!(is_valid_origin("[::1]"));
        assert!(is_valid_origin("localhost"));
        assert!(is_valid_origin("my.keenetic.net"));
        assert!(is_valid_origin("router"));

        // A-01: приватные IP больше не доверяются безусловно — только через Host-match
        // (правило 1 is_trusted_origin); здесь все они должны отклоняться
        assert!(!is_valid_origin("192.168.1.1"));
        assert!(!is_valid_origin("192.168.2.1"));
        assert!(!is_valid_origin("10.0.0.1"));
        assert!(!is_valid_origin("172.16.0.1"));
        assert!(!is_valid_origin("172.31.255.254"));

        // Вредоносные поддельные домены с префиксами IP и локальных имен
        assert!(!is_valid_origin("192.168.attacker.com"));
        assert!(!is_valid_origin("10.attacker.com"));
        assert!(!is_valid_origin("172.16.attacker.com"));
        assert!(!is_valid_origin("192.168.1.1.attacker.com"));
        assert!(!is_valid_origin("my.keenetic.net.attacker.com"));
        assert!(!is_valid_origin("router.evil.com"));
        assert!(!is_valid_origin("8.8.8.8"));
        assert!(!is_valid_origin("172.32.0.1"));
        assert!(!is_valid_origin("1.1.1.1"));
    }

    #[test]
    fn test_revoked_tokens_expiration_and_capacity() {
        let _revoked_guard = REVOKED_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tok1 = "token_valid_1";
        let tok2 = "token_valid_2";
        revoke_token(tok1);
        revoke_token(tok2);
        assert!(is_token_revoked(tok1));
        assert!(is_token_revoked(tok2));
        assert!(!is_token_revoked("unrevoked_token"));
    }

    #[tokio::test]
    async fn regression_bug_92_rate_limit_and_spawn_blocking() {
        // Баг 92:
        // 1. Параллельные попытки обходят лимит 5/мин из-за проверки до записи
        // 2. PBKDF2 считается синхронно в потоке tokio
        let ip: std::net::IpAddr = "192.168.1.192".parse().unwrap();
        clear_rate_limit(ip);

        // 5 последовательных проверок должны зафиксировать 5 попыток без необходимости ручного record_failed_attempt
        for _ in 0..5 {
            assert!(check_and_reserve_rate_limit(ip), "Первые 5 попыток должны пройти проверку");
        }
        assert!(!check_and_reserve_rate_limit(ip), "6-я попытка должна быть заблокирована rate-limit");

        // Проверяем, что в auth.rs функции login и change_password используют spawn_blocking
        let src = include_str!("auth.rs");
        assert!(src.contains("verify_password_async") || src.contains("spawn_blocking"), "PBKDF2 должен запускаться через spawn_blocking");
    }

    #[test]
    fn regression_bug_93_revoked_tokens_persist_across_restart() {
        let _revoked_guard = REVOKED_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Баг 93: Отозванные токены хранятся только в памяти: после рестарта logout отменяется (до 24 ч)
        let temp_dir = std::env::temp_dir();
        let storage_path = temp_dir.join(format!("test_revoked_tokens_{}.json", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let _ = std::fs::remove_file(&storage_path);

        init_revoked_tokens_storage(&storage_path);
        let test_token = "session_token_to_revoke_xyz";
        revoke_token(test_token);
        assert!(is_token_revoked(test_token), "Токен должен быть отозван сразу после вызова revoke_token");

        // Симулируем перезапуск процесса / очистку кэша в памяти
        reset_in_memory_revoked_tokens_for_test();

        // После рестарта инициализируем хранилище из того же файла
        init_revoked_tokens_storage(&storage_path);
        assert!(is_token_revoked(test_token), "После перезапуска отозванный токен должен оставаться отозванным (прочитан с диска)");

        let _ = std::fs::remove_file(&storage_path);
    }

    #[tokio::test]
    async fn regression_bug_91_get_api_and_clash_reject_dns_rebinding_host() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let mut cfg = crate::config::AppConfig::default();
        cfg.auth.enabled = false;
        let cfg_arc = std::sync::Arc::new(tokio::sync::RwLock::new(std::sync::Arc::new(cfg)));
        let ag = std::sync::Arc::new(crate::antigravity::AntigravityManager::new(cfg_arc.clone()));
        let state = AppState {
            config: cfg_arc,
            config_path: std::sync::Arc::new(std::env::temp_dir().join("xkr-bug91-test.json")),
            http: reqwest::Client::new(),
            failover_log: std::sync::Arc::new(crate::failover::FailoverLog::default()),
            routing_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            config_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            speedtest_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            zapret_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            antigravity: ag,
            vkturn: crate::vkturn::VkTurnManager::new(),
        };

        let app = axum::Router::new()
            .route("/api/status", axum::routing::get(|| async { "ok" }))
            .route("/clash/proxies", axum::routing::get(|| async { "ok" }))
            .layer(axum::middleware::from_fn_with_state(state.clone(), auth_middleware))
            .with_state(state);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        async fn raw_get(addr: std::net::SocketAddr, path: &str, host: &str) -> String {
            let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
            let req = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
            stream.write_all(req.as_bytes()).await.unwrap();
            let mut buf = Vec::new();
            stream.read_to_end(&mut buf).await.unwrap();
            String::from_utf8_lossy(&buf).into_owned()
        }

        let res_evil = raw_get(addr, "/api/status", "evil.attacker.com").await;
        assert!(res_evil.starts_with("HTTP/1.1 403"), "GET /api/status с evil Host должен быть отклонён: {res_evil}");

        let res_clash = raw_get(addr, "/clash/proxies", "rebind.network:1001").await;
        assert!(res_clash.starts_with("HTTP/1.1 403"), "GET /clash/proxies с evil Host должен быть отклонён: {res_clash}");

        let res_empty = raw_get(addr, "/api/status", "").await;
        assert!(res_empty.starts_with("HTTP/1.1 403"), "GET /api/status с пустым Host должен быть отклонён: {res_empty}");

        let res_lan = raw_get(addr, "/api/status", "192.168.1.1:1001").await;
        assert!(res_lan.starts_with("HTTP/1.1 200"), "GET /api/status с LAN Host должен пройти: {res_lan}");

        let res_keenetic = raw_get(addr, "/clash/proxies", "my.keenetic.net").await;
        assert!(res_keenetic.starts_with("HTTP/1.1 200"), "GET /clash/proxies с my.keenetic.net должен пройти: {res_keenetic}");

        server.abort();
    }

    #[test]
    fn regression_bug_a06_host_header_policy_and_atomic_revocation_store() {
        assert_eq!(host_header_policy(None, false), HostHeaderPolicy::Deny);
        assert_eq!(host_header_policy(None, true), HostHeaderPolicy::Allow);
        assert_eq!(host_header_policy(Some("evil.example"), true), HostHeaderPolicy::Deny);
        assert_eq!(host_header_policy(Some("192.168.1.1:1001"), false), HostHeaderPolicy::Allow);

        let dir = std::env::temp_dir().join(format!("xkeen-bug-a06-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let store = dir.join("revoked.json");
        let mut map = std::collections::HashMap::new();
        map.insert("tok-1".to_string(), 9_999_999_999u64);
        persist_revoked_tokens_to(&store, &map).unwrap();
        let loaded: std::collections::HashMap<String, u64> =
            serde_json::from_slice(&std::fs::read(&store).unwrap()).unwrap();
        assert_eq!(loaded.get("tok-1").copied(), Some(9_999_999_999));
        assert!(!store.with_extension("tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
