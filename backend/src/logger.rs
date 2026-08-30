//! Простой файловый логгер с ротацией и опциональной отправкой на удалённый syslog (UDP).
//!
//! Лог: <каталог конфига>/xkeen-route.log, при превышении 2 МБ переименовывается в .old.
//! Отправка: если в конфиге задан logs.remote_syslog ("host:port"), каждая строка
//! дублируется UDP-пакетом в формате RFC 3164 (facility local0, priority по уровню).

use std::net::UdpSocket;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const MAX_BYTES: u64 = 2 * 1024 * 1024;

static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();
static SYSLOG: OnceLock<Option<SyslogTarget>> = OnceLock::new();
static MIN_LEVEL: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0); // 0=info 1=warn 2=error

fn level_num(level: &str) -> u8 {
    match level {
        "WARN" => 1,
        "ERROR" => 2,
        _ => 0,
    }
}

/// Установить минимальный уровень ("info"|"warn"|"error").
pub fn set_level(level: &str) {
    let n = match level.to_ascii_lowercase().as_str() {
        "warn" | "warning" => 1,
        "error" => 2,
        _ => 0,
    };
    MIN_LEVEL.store(n, std::sync::atomic::Ordering::Relaxed);
}

struct SyslogTarget {
    socket: UdpSocket,
    dest: String,
    host: String,
}

pub fn init(config_path: &Path, remote_syslog: &str) {
    let dir = config_path.parent().unwrap_or_else(|| Path::new("."));
    let path = dir.join("xkeen-route.log");
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = LOG_PATH.set(path);

    let target = remote_syslog.trim().split_once(':').and_then(|(host, port)| {
        let port: u16 = port.parse().ok()?;
        let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
        Some(SyslogTarget {
            dest: format!("{host}:{port}"),
            host: host.to_string(),
            socket,
        })
    });
    let _ = SYSLOG.set(target);
}

pub fn log(level: &str, msg: &str) {
    // NOTE (known limitation): блокирующий std::fs I/O в контексте tokio worker.
    // Строки короткие, запись редкая, ротация раз в 2 МБ — для роутера приемлемо.
    // При необходимости можно перевести на mpsc + фоновую задачу записи.
    if level_num(level) < MIN_LEVEL.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    let line = format!("{ts} [{level:<5}] {msg}");

    if level == "ERROR" || level == "WARN" {
        eprintln!("{line}");
    }

    if let Some(path) = LOG_PATH.get() {
        rotate_if_needed(path);
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = f.write_all(line.as_bytes());
            let _ = f.write_all(b"\n");
        }
    }

    if let Some(Some(t)) = SYSLOG.get() {
        // RFC 3164: <priority>timestamp hostname tag: msg
        // facility local0 (16): priority = 16*8 + severity
        let pr = match level {
            "ERROR" => 131, // local0.err
            "WARN" => 132,  // local0.warning
            _ => 134,       // local0.info
        };
        let packet = format!(
            "<{pr}>{} {host} xkeen-route: {msg}",
            chrono::Local::now().format("%b %d %H:%M:%S"),
            host = t.host
        );
        let _ = t.socket.send_to(packet.as_bytes(), &t.dest);
    }
}

fn rotate_if_needed(path: &Path) {
    if let Ok(meta) = std::fs::metadata(path) {
        if meta.len() > MAX_BYTES {
            let old = path.with_extension("log.old");
            let _ = std::fs::remove_file(&old);
            let _ = std::fs::rename(path, &old);
        }
    }
}

/// Хвост лога: последние `lines` строк (по умолчанию 500, максимум 5000).
pub fn tail(lines: usize) -> Result<String, String> {
    let path = LOG_PATH
        .get()
        .ok_or_else(|| "лог не инициализирован".to_string())?;
    let content = std::fs::read_to_string(path).unwrap_or_default();
    let take = lines.clamp(1, 5000);
    let mut start = content.len().saturating_sub(take * 200);
    // Сдвигаемся вперёд до границы UTF-8 (кириллица — 2 байта, иначе slice паникует).
    while start < content.len() && !content.is_char_boundary(start) {
        start += 1;
    }
    let slice = &content[start..];
    let mut iter: Vec<&str> = slice.lines().collect();
    if iter.len() > take {
        iter = iter.split_off(iter.len() - take);
    }
    Ok(iter.join("\n"))
}

pub fn path() -> Option<&'static PathBuf> {
    LOG_PATH.get()
}

pub fn clear() -> Result<(), String> {
    let path = LOG_PATH
        .get()
        .ok_or_else(|| "лог не инициализирован".to_string())?;
    std::fs::write(path, "").map_err(|e| e.to_string())
}

/// Полное содержимое текущего лога (для скачивания).
pub fn read_all() -> Result<String, String> {
    let path = LOG_PATH
        .get()
        .ok_or_else(|| "лог не инициализирован".to_string())?;
    std::fs::read_to_string(path).map_err(|e| e.to_string())
}

#[macro_export]
macro_rules! log_i {
    ($($arg:tt)*) => { $crate::logger::log("INFO", &format!($($arg)*)) };
}
#[macro_export]
macro_rules! log_w {
    ($($arg:tt)*) => { $crate::logger::log("WARN", &format!($($arg)*)) };
}
#[macro_export]
macro_rules! log_e {
    ($($arg:tt)*) => { $crate::logger::log("ERROR", &format!($($arg)*)) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_handles_multibyte_boundary() {
        let dir = std::env::temp_dir().join("xr-log-test");
        let cfg = dir.join("config.json");
        let _ = std::fs::remove_dir_all(&dir);
        init(&cfg, "");
        // Кириллица (2 байта/символ) — старый код срезал середину символа и паниковал.
        for i in 0..300 {
            log("INFO", &format!("Проверка кириллицы {i} — тестовая строка журнала"));
        }
        let out = tail(1).expect("tail");
        assert!(out.contains("Проверка кириллицы"), "OUT={out}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}