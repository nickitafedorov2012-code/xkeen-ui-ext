use std::time::Instant;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use super::*;

pub(crate) static LAST_ACTIVATED_SERVER: Mutex<Option<(String, Instant, bool)>> = Mutex::const_new(None);

pub const EARLY_FAILURE_WINDOW_SECS: u64 = 300; // 5 минут после переключения

// ============================================================================
// FO-QUR-01: Persistent Quarantine Data Structures & Paths
// ============================================================================

#[cfg(all(target_os = "linux", not(test)))]
pub const DEFAULT_QUARANTINE_FILE: &str = "/opt/etc/xkeen-route/quarantine.json";

pub const QUARANTINE_AMNESTY_SECS: u64 = 43200; // 12 часов до полной амнистии

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistentQuarantineEntry {
    #[serde(default)]
    pub server_name: String,
    pub quarantined_at: u64,
    pub fail_count: u32,
    pub duration_secs: u64,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QuarantineInfo {
    pub server_id: String,
    pub server_name: String,
    pub fail_count: u32,
    pub remaining_secs: u64,
    pub reason: String,
}

pub(crate) static SERVER_QUARANTINE: std::sync::LazyLock<Mutex<std::collections::HashMap<String, PersistentQuarantineEntry>>> =
    std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

static QUARANTINE_SAVE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static QUARANTINE_PATH_OVERRIDE: std::sync::Mutex<Option<std::path::PathBuf>> = std::sync::Mutex::new(None);

pub fn current_unix_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn quarantine_path(config_path: &std::path::Path) -> std::path::PathBuf {
    let dir = config_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    dir.join("quarantine.json")
}

pub fn init_quarantine_path(config_path: &std::path::Path) {
    let mut guard = QUARANTINE_PATH_OVERRIDE.lock().unwrap();
    *guard = Some(quarantine_path(config_path));
}

pub fn get_quarantine_path() -> std::path::PathBuf {
    let guard = QUARANTINE_PATH_OVERRIDE.lock().unwrap();
    if let Some(ref p) = *guard {
        p.clone()
    } else {
        #[cfg(all(target_os = "linux", not(test)))]
        {
            std::path::PathBuf::from(DEFAULT_QUARANTINE_FILE)
        }
        #[cfg(any(not(target_os = "linux"), test))]
        {
            std::env::temp_dir().join(format!("xkeen-route-quarantine-{}.json", std::process::id()))
        }
    }
}

#[cfg(test)]
pub fn set_quarantine_path_override(path: Option<std::path::PathBuf>) {
    let mut guard = QUARANTINE_PATH_OVERRIDE.lock().unwrap();
    *guard = path;
}

/// Загрузка записей карантина из файла с авто-очисткой просроченных записей старше 12 часов.
pub async fn load_quarantine_from_file(
    path: impl AsRef<std::path::Path>,
) -> Result<std::collections::HashMap<String, PersistentQuarantineEntry>, String> {
    load_quarantine_from_file_at(path, current_unix_timestamp()).await
}

/// Детерминированная загрузка карантина с диска относительно переданного момента `now` (без обращения к SystemTime::now).
pub async fn load_quarantine_from_file_at(
    path: impl AsRef<std::path::Path>,
    now: u64,
) -> Result<std::collections::HashMap<String, PersistentQuarantineEntry>, String> {
    let p = path.as_ref();
    if !p.exists() {
        return Ok(std::collections::HashMap::new());
    }

    let content = tokio::fs::read_to_string(p)
        .await
        .map_err(|e| format!("Ошибка чтения {}: {e}", p.display()))?;

    if content.trim().is_empty() {
        return Ok(std::collections::HashMap::new());
    }

    let raw_entries: std::collections::HashMap<String, PersistentQuarantineEntry> =
        serde_json::from_str(&content)
            .map_err(|e| format!("Ошибка парсинга JSON из {}: {e}", p.display()))?;

    let mut clean_map = std::collections::HashMap::new();

    for (server_id, entry) in raw_entries {
        // Если запись старше 12 часов непрерывной стабильности — полная амнистия, запись удаляется
        if now.saturating_sub(entry.quarantined_at) > QUARANTINE_AMNESTY_SECS {
            continue;
        }
        clean_map.insert(server_id, entry);
    }

    Ok(clean_map)
}

/// Атомарное сохранение записей карантина на диск через временный файл и rename
pub async fn save_quarantine_to_file(
    path: impl AsRef<std::path::Path>,
    entries: &std::collections::HashMap<String, PersistentQuarantineEntry>,
) -> Result<(), String> {
    let _file_guard = QUARANTINE_SAVE_LOCK.lock().await;
    let json = serde_json::to_string_pretty(entries)
        .map_err(|e| format!("Ошибка сериализации карантина: {e}"))?;
    crate::api::atomic_write_file(path.as_ref(), &json).await
}

/// Асинхронное фоновое сохранение снимка карантина
pub fn spawn_quarantine_save(snapshot: std::collections::HashMap<String, PersistentQuarantineEntry>) {
    let path = get_quarantine_path();
    tokio::spawn(async move {
        if let Err(e) = save_quarantine_to_file(&path, &snapshot).await {
            crate::log_w!("Failover: ошибка сохранения карантина в {}: {e}", path.display());
        }
    });
}

/// Принудительный сброс текущего состояния карантина на диск (для shutdown / тестов)
#[cfg(test)]
pub async fn flush_quarantine_to_disk() -> Result<(), String> {
    let path = get_quarantine_path();
    let snapshot = {
        let q = SERVER_QUARANTINE.lock().await;
        q.clone()
    };
    save_quarantine_to_file(&path, &snapshot).await
}

/// Инициализация подсистемы карантина при старте сервиса
pub async fn init_quarantine_from_disk() -> Result<(), String> {
    let path = get_quarantine_path();
    match load_quarantine_from_file(&path).await {
        Ok(entries) => {
            let total = entries.len();
            let now = current_unix_timestamp();
            let active = entries.values().filter(|e| now < e.quarantined_at + e.duration_secs).count();
            let mut q = SERVER_QUARANTINE.lock().await;
            *q = entries;
            crate::log_i!(
                "Failover: загружено {} записей карантина из {} (активных: {}, в амнистии: {})",
                total, path.display(), active, total.saturating_sub(active)
            );
            Ok(())
        }
        Err(e) => {
            crate::log_w!(
                "Failover: повреждённый файл карантина {} ({}), инициализация с пустым карантином",
                path.display(), e
            );
            let mut q = SERVER_QUARANTINE.lock().await;
            q.clear();
            Ok(())
        }
    }
}

pub async fn is_quarantined(server_id: &str) -> bool {
    let mut q = SERVER_QUARANTINE.lock().await;
    let now = current_unix_timestamp();
    if let Some(entry) = q.get(server_id) {
        if now < entry.quarantined_at + entry.duration_secs {
            true
        } else {
            // Не удаляем сразу fail_count, чтобы сохранялась история для прогрессивного бана.
            // Полная амнистия наступает только через 12 часов непрерывной стабильности.
            if now.saturating_sub(entry.quarantined_at) > QUARANTINE_AMNESTY_SECS {
                q.remove(server_id);
            }
            false
        }
    } else {
        false
    }
}

pub async fn get_quarantine_remaining(server_id: &str) -> Option<u64> {
    let mut q = SERVER_QUARANTINE.lock().await;
    let now = current_unix_timestamp();
    if let Some(entry) = q.get(server_id) {
        if now < entry.quarantined_at + entry.duration_secs {
            Some((entry.quarantined_at + entry.duration_secs) - now)
        } else {
            if now.saturating_sub(entry.quarantined_at) > QUARANTINE_AMNESTY_SECS {
                q.remove(server_id);
            }
            None
        }
    } else {
        None
    }
}

pub async fn add_quarantine(server_id: &str, server_name: &str, reason: &str) -> u64 {
    let now = current_unix_timestamp();
    let (duration_secs, snapshot) = {
        let mut q = SERVER_QUARANTINE.lock().await;
        let prev_count = q.get(server_id)
            .filter(|e| now.saturating_sub(e.quarantined_at) <= QUARANTINE_AMNESTY_SECS)
            .map(|e| e.fail_count)
            .unwrap_or(0);
        let fail_count = prev_count + 1;
        // Эскалация: 1-й сбой: 15 мин (900с), 2-й: 30 мин (1800с), 3-й+: максимум 1 час (3600с)
        // Строго по указанию пользователя: "больше часа не надо бана"
        let duration_secs = match fail_count {
            1 => 900,
            2 => 1800,
            _ => 3600,
        };
        q.insert(server_id.to_string(), PersistentQuarantineEntry {
            server_name: server_name.to_string(),
            quarantined_at: now,
            fail_count,
            duration_secs,
            reason: reason.to_string(),
        });
        (duration_secs, q.clone())
    };
    spawn_quarantine_save(snapshot);
    duration_secs
}

pub async fn add_quarantine_with_duration(
    server_id: &str,
    server_name: &str,
    reason: &str,
    duration_secs: u64,
) -> u64 {
    let now = current_unix_timestamp();
    let capped_duration = std::cmp::min(duration_secs, 3600);
    let (duration, snapshot) = {
        let mut q = SERVER_QUARANTINE.lock().await;
        let prev_count = q.get(server_id)
            .filter(|e| now.saturating_sub(e.quarantined_at) <= QUARANTINE_AMNESTY_SECS)
            .map(|e| e.fail_count)
            .unwrap_or(0);
        let fail_count = prev_count + 1;
        q.insert(server_id.to_string(), PersistentQuarantineEntry {
            server_name: server_name.to_string(),
            quarantined_at: now,
            fail_count,
            duration_secs: capped_duration,
            reason: reason.to_string(),
        });
        (capped_duration, q.clone())
    };
    spawn_quarantine_save(snapshot);
    duration
}

pub async fn get_zombie_servers() -> Vec<String> {
    let q = SERVER_QUARANTINE.lock().await;
    let now = current_unix_timestamp();
    q.iter()
        .filter(|(_, v)| {
            if now >= v.quarantined_at + v.duration_secs {
                return false;
            }
            let r = v.reason.to_lowercase();
            // Истинный зомби: подтверждённая аномалия "Чёрная дыра" (DPI drop) или явный зомби-сервер.
            // Обычный таймаут или L7-сбой отдельного проверочного URL НЕ является зомби!
            r.contains("зомби") || r.contains("zombie") || r.contains("черн") || r.contains("blackhole")
        })
        .map(|(k, _)| k.clone())
        .collect()
}

pub async fn clear_quarantine(server_id: &str) {
    let snapshot = {
        let mut q = SERVER_QUARANTINE.lock().await;
        q.remove(server_id);
        q.clone()
    };
    spawn_quarantine_save(snapshot);
}

pub async fn clear_all_quarantine() {
    let snapshot = {
        let mut q = SERVER_QUARANTINE.lock().await;
        q.clear();
        q.clone()
    };
    spawn_quarantine_save(snapshot);
}

pub async fn quarantine_status() -> Vec<QuarantineInfo> {
    let mut q = SERVER_QUARANTINE.lock().await;
    let now = current_unix_timestamp();
    q.retain(|_, v| now < v.quarantined_at + v.duration_secs || now.saturating_sub(v.quarantined_at) <= QUARANTINE_AMNESTY_SECS);
    let mut res: Vec<QuarantineInfo> = q.iter()
        .filter(|(_, v)| now < v.quarantined_at + v.duration_secs)
        .map(|(id, v)| QuarantineInfo {
            server_id: id.clone(),
            server_name: if v.server_name.is_empty() { id.clone() } else { v.server_name.clone() },
            fail_count: v.fail_count,
            remaining_secs: (v.quarantined_at + v.duration_secs).saturating_sub(now),
            reason: v.reason.clone(),
        })
        .collect();
    res.sort_by(|a, b| a.remaining_secs.cmp(&b.remaining_secs));
    res
}

pub async fn record_switch_target(target_id: &str) {
    let now = Instant::now();
    *LAST_SWITCH.lock().await = Some(now);
    *LAST_ACTIVATED_SERVER.lock().await = Some((target_id.to_string(), now, false));
    reset_canary().await;
}

pub async fn record_manual_switch(target_id: &str) {
    let now = Instant::now();
    *LAST_SWITCH.lock().await = Some(now);
    *LAST_ACTIVATED_SERVER.lock().await = Some((target_id.to_string(), now, true));
    reset_canary().await;
    clear_quarantine(target_id).await;
}
