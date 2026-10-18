//! Простой файловый логгер с ротацией и опциональной отправкой на удалённый syslog (UDP).
//!
//! Лог: <каталог конфига>/xkeen-route.log, при превышении 2 МБ переименовывается в .old.
//! Отправка: если в конфиге задан logs.remote_syslog ("host:port"), каждая строка
//! дублируется UDP-пакетом в формате RFC 3164 (facility local0, priority по уровню).

use std::collections::VecDeque;
use std::net::UdpSocket;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use serde::{Deserialize, Serialize};

const MAX_BYTES: u64 = 2 * 1024 * 1024;
pub const RING_BUFFER_CAP: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: String,
    pub message: String,
}

static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();
static SYSLOG: OnceLock<Option<SyslogTarget>> = OnceLock::new();
static LOG_TX: OnceLock<tokio::sync::broadcast::Sender<String>> = OnceLock::new();
static FILE_TX: OnceLock<std::sync::mpsc::SyncSender<String>> = OnceLock::new();
static MIN_LEVEL: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0); // 0=info 1=warn 2=error
static RING_BUFFER: OnceLock<Mutex<VecDeque<LogEntry>>> = OnceLock::new();

fn ring_buffer() -> &'static Mutex<VecDeque<LogEntry>> {
    RING_BUFFER.get_or_init(|| Mutex::new(VecDeque::with_capacity(RING_BUFFER_CAP)))
}

/// Подписка на живой поток строк журнала (для WebSocket).
pub fn subscribe() -> Option<tokio::sync::broadcast::Receiver<String>> {
    LOG_PATH.get()?; // лог должен быть инициализирован
    let tx = LOG_TX.get_or_init(|| tokio::sync::broadcast::channel(512).0);
    Some(tx.subscribe())
}

pub fn level_num(level: &str) -> u8 {
    match level.to_ascii_uppercase().as_str() {
        "DEBUG" => 0,
        "WARN" | "WARNING" => 2,
        "ERROR" => 3,
        _ => 1,
    }
}

/// Установить минимальный уровень ("debug"|"info"|"warn"|"error").
pub fn set_level(level: &str) {
    let n = match level.to_ascii_lowercase().as_str() {
        "debug" => 0,
        "warn" | "warning" => 2,
        "error" => 3,
        _ => 1,
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
    let _ = LOG_PATH.set(path.clone());

    // Неблокирующая очередь для записи логов в отдельном потоке
    let (tx, rx) = std::sync::mpsc::sync_channel::<String>(1024);
    let _ = FILE_TX.set(tx);
    let log_path_clone = path;
    let _ = std::thread::Builder::new()
        .name("xr-logger".into())
        .spawn(move || {
            use std::io::Write;
            let mut count = 0u32;
            while let Ok(line) = rx.recv() {
                count = count.saturating_add(1);
                if count % 16 == 1 {
                    rotate_if_needed(&log_path_clone);
                }
                if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&log_path_clone) {
                    let _ = f.write_all(line.as_bytes());
                    let _ = f.write_all(b"\n");
                }
            }
        });

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
    if level_num(level) < MIN_LEVEL.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    let line = format!("{ts} [{level:<5}] {msg}");

    // Запись в in-memory кольцевой буфер (до 200 последних записей)
    if let Ok(mut rb) = ring_buffer().lock() {
        if rb.len() >= RING_BUFFER_CAP {
            rb.pop_front();
        }
        rb.push_back(LogEntry {
            timestamp: ts.to_string(),
            level: level.to_string(),
            message: msg.to_string(),
        });
    }

    // Живой поток для WebSocket-подписчиков (медленных — откидываем, не блокируем).
    if let Some(tx) = LOG_TX.get() {
        let _ = tx.send(line.clone());
    }

    if level == "ERROR" || level == "WARN" {
        eprintln!("{line}");
    }

    // Асинхронная запись в файл без блокировки Tokio рантайма
    if let Some(tx) = FILE_TX.get() {
        let _ = tx.try_send(line);
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
/// Читает с конца файла только нужный объём байт (lines * 200), не загружая
/// весь файл в память — важно для роутеров с 64 МБ RAM.
pub fn tail(lines: usize) -> Result<String, String> {
    use std::io::{Read, Seek, SeekFrom};
    let path = LOG_PATH
        .get()
        .ok_or_else(|| "лог не инициализирован".to_string())?;
    let take = lines.clamp(1, 5000);
    let want = (take * 200) as u64; // ~200 байт на строку с запасом
    let len = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    if len == 0 {
        return Ok(String::new());
    }
    let start = len.saturating_sub(want);

    let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    f.seek(SeekFrom::Start(start)).map_err(|e| e.to_string())?;
    let mut bytes = Vec::with_capacity(want as usize);
    f.read_to_end(&mut bytes).map_err(|e| e.to_string())?;

    // Читаем с середины файла — отбрасываем первую (возможно обрезанную) строку.
    if start > 0 {
        if let Some(pos) = bytes.iter().position(|&b| b == b'\n') {
            bytes.drain(..=pos);
        }
    }
    // Страховка границы UTF-8 (кириллица — 2 байта/символ).
    let mut s = 0;
    while s < bytes.len() && (bytes[s] & 0xC0) == 0x80 {
        s += 1;
    }
    let content = String::from_utf8_lossy(&bytes[s..]);
    let mut iter: Vec<&str> = content.lines().collect();
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

/// Фильтрация и ограничение последних логов по уровню.
pub fn filter_recent_logs<'a>(
    iter: impl DoubleEndedIterator<Item = &'a LogEntry>,
    level_filter: Option<&str>,
    limit: usize,
) -> Vec<LogEntry> {
    let limit = limit.clamp(1, RING_BUFFER_CAP);
    let min_lvl = level_filter
        .and_then(|l| {
            let trimmed = l.trim();
            if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("all") {
                None
            } else {
                Some(level_num(trimmed))
            }
        })
        .unwrap_or(0);

    iter.rev()
        .filter(|e| level_num(&e.level) >= min_lvl)
        .take(limit)
        .cloned()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

/// Получить последние записи логов из in-memory кольцевого буфера.
/// Опционально фильтрует по минимальному уровню (debug, info, warn, error).
/// limit ограничивается диапазоном 1..RING_BUFFER_CAP (по умолчанию 50).
pub fn get_recent_logs(level_filter: Option<&str>, limit: usize) -> Vec<LogEntry> {
    if let Ok(rb) = ring_buffer().lock() {
        filter_recent_logs(rb.iter(), level_filter, limit)
    } else {
        Vec::new()
    }
}

pub fn push_to_ring_buffer_deque(rb: &mut VecDeque<LogEntry>, entry: LogEntry) {
    if rb.len() >= RING_BUFFER_CAP {
        rb.pop_front();
    }
    rb.push_back(entry);
}

pub fn clear_ring_buffer() {
    if let Ok(mut rb) = ring_buffer().lock() {
        rb.clear();
    }
}

#[macro_export]
macro_rules! log_d {
    ($($arg:tt)*) => { $crate::logger::log("DEBUG", &format!($($arg)*)) };
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
        // Даём фоновому потоку время записать строки на диск под параллельной нагрузкой
        let mut out = String::new();
        for _ in 0..50 {
            std::thread::sleep(std::time::Duration::from_millis(20));
            if let Ok(s) = tail(50) {
                if !s.is_empty() {
                    out = s;
                    break;
                }
            }
        }
        assert!(out.contains("Проверка кириллицы"), "OUT={out}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_recent_logs_ring_buffer_capacity() {
        let mut test_rb = VecDeque::with_capacity(RING_BUFFER_CAP);

        // 1. Заполняем кольцевой буфер 250 записями (превышая RING_BUFFER_CAP = 200)
        for i in 0..250 {
            let level = if i % 10 == 0 {
                "ERROR"
            } else if i % 5 == 0 {
                "WARN"
            } else {
                "INFO"
            };
            push_to_ring_buffer_deque(&mut test_rb, LogEntry {
                timestamp: format!("2026-10-18 00:00:{:02}.000", i % 60),
                level: level.to_string(),
                message: format!("Лог сообщение #{i}"),
            });
        }

        // 2. Проверяем емкость: должно остаться ровно 200 записей
        assert_eq!(test_rb.len(), 200);

        let all_recent = filter_recent_logs(test_rb.iter(), None, 300);
        assert_eq!(all_recent.len(), 200);

        // Старые сообщения 0..49 вытеснены, самое старое в буфере должно быть #50
        assert_eq!(all_recent.first().unwrap().message, "Лог сообщение #50");
        assert_eq!(all_recent.last().unwrap().message, "Лог сообщение #249");

        // 3. Проверяем фильтрацию по уровню WARN (включает WARN и ERROR)
        let warn_logs = filter_recent_logs(test_rb.iter(), Some("WARN"), 200);
        for entry in &warn_logs {
            assert!(entry.level == "WARN" || entry.level == "ERROR");
        }
        // В диапазоне 50..250 (200 элементов):
        // Каждое 5-е сообщение — WARN или ERROR (200 / 5 = 40 сообщений)
        assert_eq!(warn_logs.len(), 40);

        // 4. Проверяем фильтрацию по уровню ERROR
        let err_logs = filter_recent_logs(test_rb.iter(), Some("ERROR"), 200);
        for entry in &err_logs {
            assert_eq!(entry.level, "ERROR");
        }
        // Каждое 10-е сообщение — ERROR (200 / 10 = 20 сообщений)
        assert_eq!(err_logs.len(), 20);

        // 5. Проверяем лимит
        let limited = filter_recent_logs(test_rb.iter(), None, 10);
        assert_eq!(limited.len(), 10);
        assert_eq!(limited.last().unwrap().message, "Лог сообщение #249");
    }
}