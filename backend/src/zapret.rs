use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use serde::{Deserialize, Serialize};

#[cfg(target_os = "linux")]
pub const ZAPRET_LOCK_FILE: &str = "/opt/var/run/xkeen-zapret.lock";
#[cfg(not(target_os = "linux"))]
pub const ZAPRET_LOCK_FILE: &str = "xkeen-zapret.lock";

#[cfg(target_os = "linux")]
pub const ZAPRET_LOCK_DIR: &str = "/opt/var/run/xkeen-zapret.lock.d";
#[cfg(not(target_os = "linux"))]
pub const ZAPRET_LOCK_DIR: &str = "xkeen-zapret.lock.d";

#[cfg(target_os = "linux")]
pub const ZAPRET_DISABLED_MARKER: &str = "/opt/var/run/xkeen-zapret.disabled";
#[cfg(not(target_os = "linux"))]
pub const ZAPRET_DISABLED_MARKER: &str = "xkeen-zapret.disabled";

/// Операционное состояние перехода службы Zapret (Z-01)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZapretOperationState {
    Idle,
    Starting,
    Stopping,
    Restarting,
    Reloading,
    Configuring,
    Upgrading,
}

impl Default for ZapretOperationState {
    fn default() -> Self {
        ZapretOperationState::Idle
    }
}

/// Разделение желаемого (desired), наблюдаемого (observed) и операционного (operation) состояний (Z-01)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZapretLifecycleState {
    pub desired_enabled: bool,
    pub observed_running: bool,
    pub observed_iptables: bool,
    pub observed_pid: Option<u32>,
    pub observed_cmdline: Option<String>,
    pub operation_state: ZapretOperationState,
    pub operation_in_progress: bool,
    pub dns_redirect_active: bool,
    pub dns_resolver_ready: bool,
    pub restart_required: bool,
    pub hot_reloaded: bool,
}

static CURRENT_OPERATION_STATE: std::sync::RwLock<ZapretOperationState> =
    std::sync::RwLock::new(ZapretOperationState::Idle);

pub fn get_current_operation_state() -> ZapretOperationState {
    *CURRENT_OPERATION_STATE.read().unwrap_or_else(|e| e.into_inner())
}

pub fn set_current_operation_state(state: ZapretOperationState) {
    if let Ok(mut w) = CURRENT_OPERATION_STATE.write() {
        *w = state;
    }
}

/// Межпроцессный и внутрипроцессный Lock Guard для Zapret (Z-01)
pub struct ZapretLockGuard {
    _in_process_guard: tokio::sync::OwnedMutexGuard<()>,
    lock_dir: PathBuf,
    lock_file: PathBuf,
    acquired_fs: bool,
}

impl ZapretLockGuard {
    /// Захват блокировки с таймаутом (в секундах)
    pub async fn acquire(
        in_process_lock: Arc<Mutex<()>>,
        op_state: ZapretOperationState,
        timeout_secs: u64,
    ) -> Result<Self, String> {
        let guard = in_process_lock.lock_owned().await;
        set_current_operation_state(op_state);

        let lock_dir = get_lock_dir_path();
        let lock_file = get_lock_file_path();

        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(timeout_secs);
        let mut acquired = false;

        while start.elapsed() < timeout {
            // Атомарное создание каталога блокировки
            match std::fs::create_dir(&lock_dir) {
                Ok(_) => {
                    acquired = true;
                    let _ = std::fs::write(&lock_file, std::process::id().to_string());
                    break;
                }
                Err(_) => {
                    // Каталог уже существует — проверяем, жив ли держатель блокировки
                    if let Ok(pid_str) = std::fs::read_to_string(&lock_file) {
                        if let Ok(pid) = pid_str.trim().parse::<u32>() {
                            if !is_pid_alive(pid) {
                                // Держатель блокировки погиб — сбрасываем устаревший lock
                                let _ = std::fs::remove_file(&lock_file);
                                let _ = std::fs::remove_dir(&lock_dir);
                                continue;
                            }
                        }
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                }
            }
        }

        if !acquired {
            set_current_operation_state(ZapretOperationState::Idle);
            return Err(format!(
                "Не удалось захватить межпроцессную блокировку Zapret за {} сек (занята другим процессом)",
                timeout_secs
            ));
        }

        Ok(Self {
            _in_process_guard: guard,
            lock_dir,
            lock_file,
            acquired_fs: true,
        })
    }
}

impl Drop for ZapretLockGuard {
    fn drop(&mut self) {
        if self.acquired_fs {
            let _ = std::fs::remove_file(&self.lock_file);
            let _ = std::fs::remove_dir(&self.lock_dir);
        }
        set_current_operation_state(ZapretOperationState::Idle);
    }
}

pub fn get_lock_file_path() -> PathBuf {
    #[cfg(target_os = "linux")]
    {
        PathBuf::from(ZAPRET_LOCK_FILE)
    }
    #[cfg(not(target_os = "linux"))]
    {
        std::env::temp_dir().join(ZAPRET_LOCK_FILE)
    }
}

pub fn get_lock_dir_path() -> PathBuf {
    #[cfg(target_os = "linux")]
    {
        PathBuf::from(ZAPRET_LOCK_DIR)
    }
    #[cfg(not(target_os = "linux"))]
    {
        std::env::temp_dir().join(ZAPRET_LOCK_DIR)
    }
}

pub fn get_disabled_marker_path() -> PathBuf {
    #[cfg(target_os = "linux")]
    {
        PathBuf::from(ZAPRET_DISABLED_MARKER)
    }
    #[cfg(not(target_os = "linux"))]
    {
        std::env::temp_dir().join(ZAPRET_DISABLED_MARKER)
    }
}

/// Проверка активности процесса по PID (Z-08)
pub fn is_pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(target_os = "linux")]
    {
        Path::new(&format!("/proc/{}", pid)).exists()
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
        false
    }
}

/// Проверка соответствия PID процессу Zapret (nfqws / nfqws2 / tpws) (Z-08)
pub fn is_zapret_pid_valid(pid: u32) -> bool {
    if !is_pid_alive(pid) {
        return false;
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(comm) = std::fs::read_to_string(format!("/proc/{}/comm", pid)) {
            let c = comm.trim().to_lowercase();
            if c == "nfqws" || c == "nfqws2" || c == "tpws" {
                return true;
            }
        }
        if let Ok(cmdline_raw) = std::fs::read(format!("/proc/{}/cmdline", pid)) {
            let cmdline = String::from_utf8_lossy(&cmdline_raw).to_lowercase();
            if cmdline.contains("nfqws") || cmdline.contains("tpws") || cmdline.contains("--qnum=200") {
                return true;
            }
        }
        false
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
        true
    }
}

/// Валидация готовности DNS-сервиса перед перенаправлением на порт 1053 (Z-02)
pub async fn check_dns_resolver_ready(target_port: u16) -> bool {
    #[cfg(target_os = "linux")]
    {
        // 1. Проверяем наличие порта в таблицах сокетов ядра (/proc/net/udp, /proc/net/tcp)
        let hex_port = format!(":{:04X}", target_port);
        let in_proc_udp = std::fs::read_to_string("/proc/net/udp")
            .map(|s| s.lines().any(|l| l.split_whitespace().nth(1).map_or(false, |addr| addr.ends_with(&hex_port))))
            .unwrap_or(false);
        let in_proc_tcp = std::fs::read_to_string("/proc/net/tcp")
            .map(|s| s.lines().any(|l| {
                let parts: Vec<&str> = l.split_whitespace().collect();
                // State 0A is LISTEN in /proc/net/tcp
                parts.get(1).map_or(false, |addr| addr.ends_with(&hex_port)) && parts.get(3).map_or(false, |st| *st == "0A")
            }))
            .unwrap_or(false);
        if in_proc_udp || in_proc_tcp {
            return true;
        }

        // 2. Fallback: попытка TCP соединения (TCP handshake валидирует слушателя)
        let addr = format!("127.0.0.1:{}", target_port);
        if let Ok(Ok(_)) = tokio::time::timeout(
            std::time::Duration::from_millis(300),
            tokio::net::TcpStream::connect(&addr),
        )
        .await
        {
            return true;
        }

        // 3. Fallback: попытка UDP DNS-запроса с ожиданием ответа (fail-closed если ответа нет)
        if let Ok(sock) = tokio::net::UdpSocket::bind("127.0.0.1:0").await {
            let dummy_dns_query = [
                0x12, 0x34, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x09, b'l', b'o', b'c', b'a', b'l', b'h', b'o', b's', b't', 0x00,
                0x00, 0x01, 0x00, 0x01,
            ];
            if let Ok(Ok(_)) = tokio::time::timeout(
                std::time::Duration::from_millis(200),
                sock.send_to(&dummy_dns_query, &addr),
            ).await {
                let mut buf = [0u8; 512];
                if let Ok(Ok((bytes_read, _))) = tokio::time::timeout(
                    std::time::Duration::from_millis(300),
                    sock.recv_from(&mut buf),
                ).await {
                    if bytes_read >= 12 {
                        return true;
                    }
                }
            }
        }

        false
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = target_port;
        true
    }
}

/// Адаптация таймаутов под производительность роутера и текущую нагрузку (NET-03)
pub fn get_hardware_adaptive_timeout(base: std::time::Duration) -> std::time::Duration {
    #[cfg(target_os = "linux")]
    {
        let mut multiplier = 1.0f64;

        // 1. Оценка RAM (<= 128 MB — маломощные роутеры MIPS/ARM)
        if let Ok(meminfo) = std::fs::read_to_string("/proc/meminfo") {
            for line in meminfo.lines() {
                if line.starts_with("MemTotal:") {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if let Some(kb_str) = parts.get(1) {
                        if let Ok(kb) = kb_str.parse::<u64>() {
                            if kb <= 131072 {
                                multiplier = multiplier.max(2.0);
                            }
                        }
                    }
                    break;
                }
            }
        }

        // 2. Оценка архитектуры (MIPS 580-880MHz: Keenetic Start, 4G, Omni, Extra)
        let arch = std::env::consts::ARCH.to_ascii_lowercase();
        if arch.contains("mips") {
            multiplier = multiplier.max(2.0);
        }

        // 3. Оценка текущей нагрузки (load average > 1.5)
        if let Ok(loadavg) = std::fs::read_to_string("/proc/loadavg") {
            if let Some(first) = loadavg.split_whitespace().next() {
                if let Ok(load1) = first.parse::<f64>() {
                    if load1 >= 2.0 {
                        multiplier += 1.0;
                    } else if load1 >= 1.0 {
                        multiplier += 0.5;
                    }
                }
            }
        }

        let adapted_millis = (base.as_millis() as f64 * multiplier) as u64;
        std::time::Duration::from_millis(adapted_millis)
    }
    #[cfg(not(target_os = "linux"))]
    {
        base
    }
}

/// Проверка конфигурации DNS на риск циклических петель (NET-01)
pub fn detect_dns_loop_risk(nameservers: &[String], local_ports: &[u16]) -> Result<(), String> {
    for ns in nameservers {
        let trimmed = ns.trim().trim_start_matches("udp://").trim_start_matches("tcp://");
        let host_port: Vec<&str> = trimmed.split(':').collect();
        let host = host_port.get(0).copied().unwrap_or("").trim();
        let port: u16 = host_port.get(1).and_then(|p| p.parse().ok()).unwrap_or(53);

        let is_localhost = host == "127.0.0.1" || host == "localhost" || host == "0.0.0.0" || host == "::1";
        if is_localhost && local_ports.contains(&port) {
            return Err(format!(
                "Обнаружен риск циклической петли DNS: адрес '{ns}' ссылается на локальный порт {port}, перенаправляемый системой"
            ));
        }
    }
    Ok(())
}

/// Результат многокомпонентной проверки failsafe (Z-05)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailsafeVerificationReport {
    pub router_alive: bool,
    pub internet_reachable: bool,
    pub lan_bridge_valid: bool,
    pub overall_success: bool,
    pub details: Vec<String>,
}

/// Строгая достоверная проверка Failsafe (Z-05, NET-03)
/// Не считает неизвестное состояние за успех (fail-closed).
pub async fn verify_zapret_failsafe(
    http_client: &reqwest::Client,
    router_check_url: Option<&str>,
) -> FailsafeVerificationReport {
    let mut details = Vec::new();
    let mut router_alive = false;
    let mut internet_reachable = false;
    let mut lan_bridge_valid = false;

    // 1. Проверка liveness роутера с адаптивным таймаутом (NET-03)
    let router_timeout = get_hardware_adaptive_timeout(std::time::Duration::from_secs(2));
    if let Some(r_url) = router_check_url {
        match tokio::time::timeout(
            router_timeout,
            http_client.get(r_url).send(),
        )
        .await
        {
            Ok(Ok(_)) => {
                router_alive = true;
                details.push("Роутер отвечает на запросы".into());
            }
            Ok(Err(e)) => {
                details.push(format!("Роутер недоступен: {e}"));
            }
            Err(_) => {
                details.push("Таймаут проверки доступности роутера".into());
            }
        }
    } else {
        router_alive = true;
        details.push("Проверка роутера пропущена (автономный режим)".into());
    }

    // 2. Проверка доступности интернета (Fail-Closed, без фиктивных успехов, NET-03)
    let internet_timeout = get_hardware_adaptive_timeout(std::time::Duration::from_secs(3));
    let endpoints = [
        "http://cp.cloudflare.com/generate_204",
        "http://connectivitycheck.gstatic.com/generate_204",
    ];
    for ep in endpoints {
        match tokio::time::timeout(
            internet_timeout,
            http_client.get(ep).send(),
        )
        .await
        {
            Ok(Ok(res)) => {
                let status = res.status();
                if status.as_u16() == 204 || (status.as_u16() >= 200 && status.as_u16() < 400) {
                    internet_reachable = true;
                    details.push(format!("Интернет доступен через {ep} (HTTP {})", status.as_u16()));
                    break;
                } else {
                    details.push(format!("Непредвиденный статус {ep}: HTTP {}", status.as_u16()));
                }
            }
            Ok(Err(e)) => {
                details.push(format!("Сбой обращения к {ep}: {e}"));
            }
            Err(_) => {
                details.push(format!("Таймаут обращения к {ep}"));
            }
        }
    }

    // 3. Проверка LAN-пути (наличие мостовых интерфейсов br+ / Bridge+)
    #[cfg(target_os = "linux")]
    {
        if let Ok(interfaces) = std::fs::read_to_string("/proc/net/dev") {
            if interfaces.contains("br") || interfaces.contains("Bridge") {
                lan_bridge_valid = true;
                details.push("Мостовой интерфейс LAN (br+/Bridge+) обнаружен".into());
            } else {
                details.push("Предупреждение: мостовые интерфейсы LAN не найдены в /proc/net/dev".into());
            }
        } else {
            details.push("Не удалось прочитать /proc/net/dev".into());
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        lan_bridge_valid = true;
        details.push("(Симуляция) LAN bridge валиден".into());
    }

    let overall_success = router_alive && internet_reachable && lan_bridge_valid;

    FailsafeVerificationReport {
        router_alive,
        internet_reachable,
        lan_bridge_valid,
        overall_success,
        details,
    }
}

/// Симметричная сериализация zapret.conf (Z-07)
pub fn serialize_zapret_conf(cfg: &crate::config::ZapretConfig) -> String {
    let use_v2 = crate::api::should_use_nfqws2(cfg);
    let engine_str = if use_v2 { "v2" } else { "v1" };

    let (args, voice_enabled) = if let Some(custom) = &cfg.custom_args {
        let is_full = custom.contains("--daemon") || custom.contains("--qnum");
        if use_v2 {
            if is_full {
                (custom.clone(), cfg.discord_voice_udp)
            } else {
                crate::api::build_nfqws2_args_with_desync(cfg, Some(custom.as_str()))
            }
        } else {
            if is_full && !custom.contains("lua-desync") && !custom.contains("payload=") && !custom.contains("out-range=") {
                (custom.replace("multisplit", "split2"), cfg.discord_voice_udp)
            } else {
                let legacy_desync = crate::api::convert_lua_to_legacy_desync(custom);
                crate::api::build_nfqws_args_with_desync(cfg, Some(&legacy_desync))
            }
        }
    } else if use_v2 {
        crate::api::build_nfqws2_args(cfg)
    } else {
        crate::api::build_nfqws_args(cfg)
    };

    let sanitized_args = args.replace('\r', " ").replace('\n', " ").replace('"', "");

    let excluded_ips_str = cfg
        .excluded_devices
        .iter()
        .filter(|d| !d.contains(':'))
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");

    let excluded_macs_str = cfg
        .excluded_devices
        .iter()
        .filter(|d| d.contains(':'))
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");

    let block_quic_str = if cfg.smart_tv_mode { "1" } else { "0" };
    let smart_tv_str = if cfg.smart_tv_mode { "1" } else { "0" };

    format!(
        "ZAPRET_ENGINE=\"{}\"\nNFQWS_ARGS=\"{}\"\nDISCORD_VOICE_ENABLED=\"{}\"\nBLOCK_QUIC=\"{}\"\nSMART_TV_MODE=\"{}\"\nEXCLUDED_IPS=\"{}\"\nEXCLUDED_MACS=\"{}\"\n",
        engine_str,
        sanitized_args.trim(),
        if voice_enabled { "1" } else { "0" },
        block_quic_str,
        smart_tv_str,
        excluded_ips_str.trim(),
        excluded_macs_str.trim()
    )
}

/// Симметричный парсинг zapret.conf (Z-07)
pub fn parse_zapret_conf(
    content: &str,
    base_cfg: &crate::config::ZapretConfig,
) -> crate::config::ZapretConfig {
    let mut cfg = base_cfg.clone();
    let mut parsed_nfqws_args: Option<String> = None;
    let mut excluded_ips = Vec::new();
    let mut excluded_macs = Vec::new();
    let mut seen_excluded_ips = false;
    let mut seen_excluded_macs = false;
    let mut seen_smart_tv = false;
    let mut seen_block_quic = false;
    let mut smart_tv_val = false;
    let mut block_quic_val = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = trimmed.split_once('=') {
            let key = k.trim();
            let clean_val = v.trim().trim_matches('"').trim_matches('\'').trim();
            match key {
                "ZAPRET_ENGINE" => {
                    cfg.engine = crate::api::normalize_engine_choice(clean_val).to_string();
                }
                "NFQWS_ARGS" => {
                    if !clean_val.is_empty() {
                        parsed_nfqws_args = Some(clean_val.to_string());
                    }
                }
                "DISCORD_VOICE_ENABLED" => {
                    cfg.discord_voice_udp = clean_val == "1";
                }
                "SMART_TV_MODE" => {
                    seen_smart_tv = true;
                    smart_tv_val = clean_val == "1";
                }
                "BLOCK_QUIC" => {
                    seen_block_quic = true;
                    block_quic_val = clean_val == "1";
                }
                "EXCLUDED_IPS" => {
                    seen_excluded_ips = true;
                    for ip in clean_val.split_whitespace() {
                        if !ip.is_empty() {
                            excluded_ips.push(ip.to_string());
                        }
                    }
                }
                "EXCLUDED_MACS" => {
                    seen_excluded_macs = true;
                    for mac in clean_val.split_whitespace() {
                        if !mac.is_empty() {
                            excluded_macs.push(mac.to_string());
                        }
                    }
                }
                _ => {}
            }
        }
    }

    if seen_smart_tv {
        cfg.smart_tv_mode = smart_tv_val;
    } else if seen_block_quic {
        cfg.smart_tv_mode = block_quic_val;
    }

    // Объединяем IP и MAC исключения обратно в excluded_devices (с полным сбросом если заданы пустыми)
    if seen_excluded_ips || seen_excluded_macs {
        let mut combined_exclusions = excluded_ips;
        combined_exclusions.extend(excluded_macs);
        cfg.excluded_devices = combined_exclusions;
    }

    if let Some(raw_args) = parsed_nfqws_args {
        let mut tmp_zapret = cfg.clone();
        tmp_zapret.custom_args = None;
        let (default_args, _) = if crate::api::should_use_nfqws2(&cfg) {
            crate::api::build_nfqws2_args(&tmp_zapret)
        } else {
            crate::api::build_nfqws_args(&tmp_zapret)
        };
        if raw_args.trim() == default_args.trim() {
            cfg.custom_args = None;
        } else {
            let effective = if !crate::api::should_use_nfqws2(&cfg)
                && (raw_args.contains("--lua-desync") || raw_args.contains("multisplit"))
            {
                crate::api::convert_lua_to_legacy_desync(&raw_args)
            } else {
                raw_args
            };
            cfg.custom_args = Some(effective);
        }
    }

    cfg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zapret_conf_symmetric_roundtrip() {
        let mut cfg = crate::config::ZapretConfig::default();
        cfg.engine = "v2".to_string();
        cfg.smart_tv_mode = false;
        cfg.discord_voice_udp = true;
        cfg.excluded_devices = vec!["192.168.1.50".to_string(), "00:11:22:33:44:55".to_string()];

        let serialized = serialize_zapret_conf(&cfg);
        assert!(serialized.contains("SMART_TV_MODE=\"0\""));
        assert!(serialized.contains("EXCLUDED_IPS=\"192.168.1.50\""));
        assert!(serialized.contains("EXCLUDED_MACS=\"00:11:22:33:44:55\""));

        let parsed = parse_zapret_conf(&serialized, &crate::config::ZapretConfig::default());
        assert_eq!(parsed.engine, "v2");
        assert_eq!(parsed.smart_tv_mode, false);
        assert_eq!(parsed.discord_voice_udp, true);
        assert_eq!(parsed.excluded_devices.len(), 2);
        assert!(parsed.excluded_devices.contains(&"192.168.1.50".to_string()));
        assert!(parsed.excluded_devices.contains(&"00:11:22:33:44:55".to_string()));
    }

    #[test]
    fn test_zapret_conf_empty_exclusions_clearing() {
        let base = crate::config::ZapretConfig {
            excluded_devices: vec!["192.168.1.50".to_string(), "00:11:22:33:44:55".to_string()],
            ..Default::default()
        };
        let conf_str = "ZAPRET_ENGINE=\"v2\"\nEXCLUDED_IPS=\"\"\nEXCLUDED_MACS=\"\"\n";
        let parsed = parse_zapret_conf(conf_str, &base);
        assert!(
            parsed.excluded_devices.is_empty(),
            "Empty EXCLUDED_IPS and EXCLUDED_MACS must clear excluded_devices"
        );
    }

    #[test]
    fn test_zapret_conf_block_quic_symmetric_off() {
        let base = crate::config::ZapretConfig {
            smart_tv_mode: true,
            ..Default::default()
        };
        let conf_str = "ZAPRET_ENGINE=\"v2\"\nBLOCK_QUIC=\"0\"\n";
        let parsed = parse_zapret_conf(conf_str, &base);
        assert_eq!(
            parsed.smart_tv_mode,
            false,
            "BLOCK_QUIC=0 without SMART_TV_MODE must symmetrically set smart_tv_mode to false"
        );
    }

    #[test]
    fn test_zapret_conf_smart_tv_mode_toggle_off() {
        let base = crate::config::ZapretConfig {
            smart_tv_mode: true,
            ..Default::default()
        };
        let conf_str = "ZAPRET_ENGINE=\"v2\"\nSMART_TV_MODE=\"0\"\nBLOCK_QUIC=\"0\"\n";
        let parsed = parse_zapret_conf(conf_str, &base);
        assert_eq!(parsed.smart_tv_mode, false, "SMART_TV_MODE=0 must symmetrically disable smart_tv_mode");
    }

    #[tokio::test]
    async fn test_zapret_lock_guard_mutual_exclusion() {
        let mutex = Arc::new(Mutex::new(()));
        let guard1 = ZapretLockGuard::acquire(mutex.clone(), ZapretOperationState::Starting, 2).await;
        assert!(guard1.is_ok());
        assert_eq!(get_current_operation_state(), ZapretOperationState::Starting);

        drop(guard1);
        assert_eq!(get_current_operation_state(), ZapretOperationState::Idle);

        let guard2 = ZapretLockGuard::acquire(mutex.clone(), ZapretOperationState::Stopping, 2).await;
        assert!(guard2.is_ok());
        assert_eq!(get_current_operation_state(), ZapretOperationState::Stopping);
    }

    #[test]
    fn test_detect_dns_loop_risk() {
        let safe_servers = vec![
            "1.1.1.1:53".to_string(),
            "https://dns.google/dns-query".to_string(),
            "77.88.8.8".to_string(),
        ];
        assert!(detect_dns_loop_risk(&safe_servers, &[1053]).is_ok());

        let loop_server1 = vec!["127.0.0.1:1053".to_string()];
        assert!(detect_dns_loop_risk(&loop_server1, &[1053]).is_err());

        let loop_server2 = vec!["udp://localhost:1053".to_string()];
        assert!(detect_dns_loop_risk(&loop_server2, &[1053]).is_err());

        let loop_server3 = vec!["127.0.0.1:53".to_string()];
        assert!(detect_dns_loop_risk(&loop_server3, &[53, 1053]).is_err());
    }

    #[test]
    fn test_hardware_adaptive_timeout() {
        let base = std::time::Duration::from_secs(2);
        let adapted = get_hardware_adaptive_timeout(base);
        // Timeout must never be smaller than base
        assert!(adapted >= base);
    }
}
