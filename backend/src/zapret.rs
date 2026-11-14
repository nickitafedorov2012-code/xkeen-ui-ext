use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
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
#[derive(Default)]
pub enum ZapretOperationState {
    #[default]
    Idle,
    Starting,
    Stopping,
    Restarting,
    Reloading,
    Configuring,
    Upgrading,
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
        let pid_file = lock_dir.join("pid");

        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(timeout_secs);
        let mut acquired = false;

        while start.elapsed() < timeout {
            // Атомарное создание каталога блокировки (единый протокол с S51zapret: lockdir/pid)
            match std::fs::create_dir(&lock_dir) {
                Ok(_) => {
                    acquired = true;
                    let _ = std::fs::write(&pid_file, std::process::id().to_string());
                    let _ = std::fs::write(&lock_file, std::process::id().to_string());
                    break;
                }
                Err(_) => {
                    // Каталог уже существует — проверяем, жив ли держатель блокировки
                    let holder_pid = std::fs::read_to_string(&pid_file)
                        .ok()
                        .or_else(|| std::fs::read_to_string(&lock_file).ok())
                        .and_then(|s| s.trim().parse::<u32>().ok());
                    // Z-14: lockdir БЕЗ читаемого pid-файла (краш между mkdir и write)
                    // старше 60 секунд считаем мусором — раньше он блокировал операции
                    // навсегда (до таймаута каждой попытки)
                    let stale = match holder_pid {
                        Some(pid) => !is_pid_alive(pid),
                        None => std::fs::metadata(&lock_dir)
                            .ok()
                            .and_then(|m| m.modified().ok())
                            .and_then(|m| std::time::SystemTime::now().duration_since(m).ok())
                            .map(|age| age.as_secs() > 60)
                            .unwrap_or(false),
                    };
                    if stale {
                        let _ = std::fs::remove_dir_all(&lock_dir);
                        let _ = std::fs::remove_file(&lock_file);
                        continue;
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
            let _ = std::fs::remove_dir_all(&self.lock_dir);
            let _ = std::fs::remove_file(&self.lock_file);
        }
        set_current_operation_state(ZapretOperationState::Idle);
    }
}

/// Маркер «Zapret выключен пользователем»: создаётся при выключении службы и
/// проверяется S51zapret при автозапуске Entware, чтобы выключенный Zapret
/// не воскресал после перезагрузки роутера (Z-03).
pub fn set_zapret_disabled_marker() {
    let _ = std::fs::write(get_disabled_marker_path(), "disabled");
}

pub fn clear_zapret_disabled_marker() {
    let _ = std::fs::remove_file(get_disabled_marker_path());
}

pub fn is_zapret_disabled_marker_set() -> bool {
    get_disabled_marker_path().exists()
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
        std::path::Path::new(&format!("/proc/{}", pid)).exists()
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
        false
    }
}

/// Единые маркеры командной строки нашего процесса Zapret для Rust и shell-скриптов (баг 36)
pub const ZAPRET_CMDLINE_MARKERS: &[&str] = &[
    "--qnum=200",
    "/opt/etc/zapret",
    "/opt/zapret",
    "xkeen-route",
];

/// Сборка shell-функции `is_zapret_pid` из единой константы маркеров `ZAPRET_CMDLINE_MARKERS` (баг 36)
pub fn build_zapret_pid_check_fn() -> String {
    build_zapret_pid_check_fn_with_qnum(200)
}

/// Сборка shell-функции `is_zapret_pid` с учётом настроенного `--qnum` (баг 34 / баг 36)
pub fn build_zapret_pid_check_fn_with_qnum(qnum: u16) -> String {
    let mut markers: Vec<String> = ZAPRET_CMDLINE_MARKERS.iter().map(|m| (*m).to_string()).collect();
    let q_eq = format!("--qnum={qnum}");
    let q_sp = format!("--qnum {qnum}");
    if !markers.contains(&q_eq) {
        markers.push(q_eq);
    }
    if !markers.contains(&q_sp) {
        markers.push(q_sp);
    }
    let patterns = markers
        .iter()
        .map(|m| format!("*{m}*"))
        .collect::<Vec<_>>()
        .join("|");
    format!(
        "is_zapret_pid() {{\n\
  local p=\"$1\"\n\
  [ -z \"$p\" ] && return 1\n\
  [ -d \"/proc/$p\" ] || return 1\n\
  if [ -r \"/proc/$p/cmdline\" ]; then\n\
    local cmdline=$(tr '\\0' ' ' < \"/proc/$p/cmdline\" 2>/dev/null)\n\
    case \"$cmdline\" in\n\
      {patterns}) return 0 ;;\n\
    esac\n\
  fi\n\
  return 1\n\
}}"
    )
}

/// Проверка, принадлежит ли командная строка процесса нашему экземпляру Zapret (баг 36)
pub fn is_zapret_cmdline_ours(cmdline: &str) -> bool {
    is_zapret_cmdline_ours_with_qnum(cmdline, None)
}

/// Проверка принадлежности процесса с учётом кастомного `--qnum` (баг 34 / баг 36)
pub fn is_zapret_cmdline_ours_with_qnum(cmdline: &str, expected_qnum: Option<u16>) -> bool {
    let cl = cmdline.to_lowercase();
    if ZAPRET_CMDLINE_MARKERS.iter().any(|m| cl.contains(m)) {
        return true;
    }
    if let Some(qnum) = expected_qnum {
        let q_eq = format!("--qnum={qnum}");
        let q_sp = format!("--qnum {qnum}");
        if cl.contains(&q_eq) || cl.contains(&q_sp) {
            return true;
        }
    }
    false
}
/// Проверка соответствия PID процессу Zapret (nfqws / nfqws2 / tpws) (Z-08)
pub fn is_zapret_pid_valid(pid: u32) -> bool {
    if !is_pid_alive(pid) {
        return false;
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(cmdline_raw) = std::fs::read(format!("/proc/{}/cmdline", pid)) {
            let cmdline = String::from_utf8_lossy(&cmdline_raw);
            if is_zapret_cmdline_ours(&cmdline) {
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

/// Выбирает из вывода `pidof` первый PID, принадлежащий нашему экземпляру Zapret (баг 36).
pub fn select_our_zapret_pid_from_list<F>(pidof_stdout: &str, mut is_valid: F) -> Option<u32>
where
    F: FnMut(u32) -> bool,
{
    pidof_stdout
        .split_whitespace()
        .filter_map(|s| s.parse::<u32>().ok())
        .find(|&p| is_valid(p))
}

/// Находит живой PID именно нашего экземпляра Zapret (nfqws / nfqws2), проверяя `/opt/var/run/zapret.pid`,
/// `/var/run/zapret.pid` и все PID из `pidof nfqws2; pidof nfqws` через `is_zapret_pid_valid` (баг 36).
pub async fn find_our_zapret_pid() -> Option<u32> {
    for pid_path in ["/opt/var/run/zapret.pid", "/var/run/zapret.pid"] {
        if let Ok(pid_txt) = tokio::fs::read_to_string(pid_path).await {
            if let Ok(p) = pid_txt.trim().parse::<u32>() {
                if is_zapret_pid_valid(p) {
                    return Some(p);
                }
            }
        }
    }
    let out = tokio::process::Command::new("sh")
        .arg("-c")
        .arg("pidof nfqws2 2>/dev/null; pidof nfqws 2>/dev/null")
        .output()
        .await
        .ok()?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    select_our_zapret_pid_from_list(&stdout, is_zapret_pid_valid)
}

// Подпункт 31.1 & 31.3: сборка безопасной команды снятия DNS-редиректов Zapret
pub fn build_dns_redirect_remove_cmd(port: u16) -> String {
    format!(
        r#"for ipt in "iptables -w 2" "ip6tables -w 2"; do
  $ipt -t nat -S PREROUTING 2>/dev/null | grep 'xkeen-route-zapret' | grep -- "--to-ports {port}" | while read -r rule; do
    del_cmd=$(echo "$rule" | sed 's/^-A/-D/')
    $ipt -t nat $del_cmd 2>/dev/null || true
  done
  for iface in "br+" "Bridge+"; do
    for proto in udp tcp; do
      while $ipt -t nat -D PREROUTING -i "$iface" -p "$proto" --dport 53 -m comment --comment "xkeen-route-zapret" -j REDIRECT --to-ports {port} 2>/dev/null; do :; done
    done
  done
done"#
    )
}

// Подпункт 31.2: проверка отсутствия правил DNS-редиректа Zapret
pub fn zapret_dns_rules_missing(iptables_s_output: &str, port: u16) -> bool {
    let port_str = format!("--to-ports {port}");
    let alt_port_str = format!("to-ports {port}");
    !iptables_s_output.lines().any(|line| {
        line.contains("xkeen-route-zapret")
            && line.contains("REDIRECT")
            && (line.contains(&port_str) || line.contains(&alt_port_str))
    })
}

// Подпункт 31.2 & 31.3: проверка остаточных правил Zapret в iptables при выключенном Zapret
pub fn build_zapret_iptables_left_check_cmd(port: u16) -> String {
    format!(
        "iptables -w 2 -t mangle -S 2>/dev/null | grep -q zapret || iptables -w 2 -t nat -S PREROUTING 2>/dev/null | grep 'xkeen-route-zapret' | grep 'to-ports {port}' | grep -q ."
    )
}

// Подпункт 31.2 & 31.3: команда восстановления правил DNS-редиректа Zapret
pub fn build_zapret_dns_restore_cmd(port: u16) -> String {
    format!(
        r#"iptables -w 2 -t nat -A PREROUTING -i br+ -p udp --dport 53 -m comment --comment "xkeen-route-zapret" -j REDIRECT --to-ports {port} 2>/dev/null || true
iptables -w 2 -t nat -A PREROUTING -i br+ -p tcp --dport 53 -m comment --comment "xkeen-route-zapret" -j REDIRECT --to-ports {port} 2>/dev/null || true
iptables -w 2 -t nat -A PREROUTING -i Bridge+ -p udp --dport 53 -m comment --comment "xkeen-route-zapret" -j REDIRECT --to-ports {port} 2>/dev/null || true
iptables -w 2 -t nat -A PREROUTING -i Bridge+ -p tcp --dport 53 -m comment --comment "xkeen-route-zapret" -j REDIRECT --to-ports {port} 2>/dev/null || true"#
    )
}



/// Валидация байтов ответа DNS: проверка длины, совпадения Transaction ID и флага QR (Response)
pub fn validate_dns_response(query_id: (u8, u8), buf: &[u8]) -> bool {
    // DNS Header: ID(2), Flags(2), QDCOUNT(2), ANCOUNT(2), NSCOUNT(2), ARCOUNT(2) = 12 bytes
    if buf.len() < 12 {
        return false;
    }
    // Проверка совпадения Transaction ID
    if buf[0] != query_id.0 || buf[1] != query_id.1 {
        return false;
    }
    // QR bit (старший бит первого байта флагов: 0 = Query, 1 = Response)
    
    (buf[2] & 0x80) != 0
}

/// Валидация готовности DNS-сервиса перед перенаправлением на порт 1053 (Z-02 & AGH-00)
pub async fn check_dns_resolver_ready(target_port: u16) -> bool {
    #[cfg(target_os = "linux")]
    {
        let addr = format!("127.0.0.1:{}", target_port);
        let query_id = (0x7A, 0x51);

        // Стандартный DNS-запрос для проверки резолвера ("localhost" A IN, RD=1)
        let dns_query = [
            query_id.0, query_id.1, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x09, b'l', b'o', b'c', b'a', b'l', b'h', b'o', b's', b't', 0x00,
            0x00, 0x01, 0x00, 0x01,
        ];

        // 1. Проверяем реальным UDP DNS-запросом к резолверу
        if let Ok(sock) = tokio::net::UdpSocket::bind("127.0.0.1:0").await {
            if let Ok(Ok(_)) = tokio::time::timeout(
                std::time::Duration::from_millis(300),
                sock.send_to(&dns_query, &addr),
            ).await {
                let mut buf = [0u8; 512];
                if let Ok(Ok((bytes_read, _))) = tokio::time::timeout(
                    std::time::Duration::from_millis(400),
                    sock.recv_from(&mut buf),
                ).await {
                    if validate_dns_response(query_id, &buf[..bytes_read]) {
                        return true;
                    }
                }
            }
        }

        // 2. Fallback: проверка по TCP (с префиксом 2-байтовой длины пакета)
        if let Ok(Ok(mut stream)) = tokio::time::timeout(
            std::time::Duration::from_millis(300),
            tokio::net::TcpStream::connect(&addr),
        ).await {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let len_prefix = (dns_query.len() as u16).to_be_bytes();
            let mut tcp_packet = Vec::with_capacity(2 + dns_query.len());
            tcp_packet.extend_from_slice(&len_prefix);
            tcp_packet.extend_from_slice(&dns_query);

            if let Ok(Ok(_)) = tokio::time::timeout(
                std::time::Duration::from_millis(300),
                stream.write_all(&tcp_packet),
            ).await {
                let mut resp_len_buf = [0u8; 2];
                if let Ok(Ok(_)) = tokio::time::timeout(
                    std::time::Duration::from_millis(400),
                    stream.read_exact(&mut resp_len_buf),
                ).await {
                    let resp_len = u16::from_be_bytes(resp_len_buf) as usize;
                    if resp_len >= 12 && resp_len <= 4096 {
                        let mut resp_buf = vec![0u8; resp_len];
                        if let Ok(Ok(_)) = tokio::time::timeout(
                            std::time::Duration::from_millis(400),
                            stream.read_exact(&mut resp_buf),
                        ).await {
                            if validate_dns_response(query_id, &resp_buf) {
                                return true;
                            }
                        }
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
        let host = host_port.first().copied().unwrap_or("").trim();
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
pub fn is_failsafe_http_status_valid(status: u16) -> bool {
    status == 204
}

pub fn has_lan_bridge_interface(net_dev_content: &str) -> bool {
    for line in net_dev_content.lines() {
        if let Some((iface, _)) = line.split_once(':') {
            let name = iface.trim();
            if name.starts_with("br") || name.starts_with("Bridge") {
                return true;
            }
        }
    }
    false
}

pub async fn verify_zapret_failsafe(
    http_client: &reqwest::Client,
    router_check_url: Option<&str>,
) -> FailsafeVerificationReport {
    let mut details = Vec::new();
    let mut router_alive = false;
    let mut internet_reachable = false;
    let lan_bridge_valid;

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
                let status = res.status().as_u16();
                if is_failsafe_http_status_valid(status) {
                    internet_reachable = true;
                    details.push(format!("Интернет доступен через {ep} (HTTP {status})"));
                    break;
                } else {
                    details.push(format!("Непредвиденный статус {ep}: HTTP {status}"));
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
            if has_lan_bridge_interface(&interfaces) {
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

/// Симметричная сериализация zapret.conf (Z-07, баг 35: единый сериализатор с параметром config_path)
pub fn serialize_zapret_conf(cfg: &crate::config::ZapretConfig, config_path: &std::path::Path) -> String {
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

    let sanitized_args = args.replace(['\r', '\n'], " ").replace('"', "");

    let excluded_ips_str = cfg
        .excluded_devices
        .iter()
        .map(|d| d.trim())
        .filter(|d| crate::config::is_valid_ip_or_cidr(d))
        .collect::<Vec<_>>()
        .join(" ");

    let excluded_macs_str = cfg
        .excluded_devices
        .iter()
        .map(|d| d.trim())
        .filter(|d| crate::config::is_valid_mac(d))
        .collect::<Vec<_>>()
        .join(" ");

    let block_quic_str = if cfg.smart_tv_mode { "1" } else { "0" };
    let smart_tv_str = if cfg.smart_tv_mode { "1" } else { "0" };
    let cfg_adguard = config_path
        .exists()
        .then(|| {
            let c = crate::config::load(config_path);
            c.adguard.enabled || c.adblock_enabled
        })
        .unwrap_or(false);
    let adguard_enabled = crate::api::zapret_conf_adguard_flag(
        cfg_adguard,
        crate::system::is_process_running("AdGuardHome"),
        std::path::Path::new("/opt/etc/init.d/S99adguardhome").exists(),
    );
    let adguard_str = if adguard_enabled { "1" } else { "0" };

    format!(
        "ZAPRET_ENGINE=\"{}\"\nNFQWS_ARGS=\"{}\"\nDISCORD_VOICE_ENABLED=\"{}\"\nBLOCK_QUIC=\"{}\"\nSMART_TV_MODE=\"{}\"\nEXCLUDED_IPS=\"{}\"\nEXCLUDED_MACS=\"{}\"\nADGUARD_ENABLED=\"{}\"\n",
        engine_str,
        sanitized_args.trim(),
        if voice_enabled { "1" } else { "0" },
        block_quic_str,
        smart_tv_str,
        excluded_ips_str.trim(),
        excluded_macs_str.trim(),
        adguard_str
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
                "ADGUARD_ENABLED" => {}
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
    fn test_zapret_disabled_marker_roundtrip() {
        // Z-03 anti-regression: маркер «выключено пользователем» обязан существовать
        // и корректно ставиться/сниматься (он читается S51zapret при автозапуске)
        clear_zapret_disabled_marker();
        assert!(!is_zapret_disabled_marker_set());
        set_zapret_disabled_marker();
        assert!(is_zapret_disabled_marker_set());
        clear_zapret_disabled_marker();
        assert!(!is_zapret_disabled_marker_set());
    }

    #[test]
    fn test_zapret_conf_symmetric_roundtrip() {
        let mut cfg = crate::config::ZapretConfig::default();
        cfg.engine = "v2".to_string();
        cfg.smart_tv_mode = false;
        cfg.discord_voice_udp = true;
        cfg.excluded_devices = vec!["192.168.1.50".to_string(), "00:11:22:33:44:55".to_string()];

        let serialized = serialize_zapret_conf(&cfg, std::path::Path::new("/opt/etc/xkeen-route/config.json"));
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
    fn test_serialize_zapret_conf_filters_injections_and_invalid_devices() {
        let mut cfg = crate::config::ZapretConfig::default();
        cfg.excluded_devices = vec![
            "192.168.1.50".to_string(),
            "10.0.0.0/24".to_string(),
            "00:11:22:33:44:55".to_string(),
            "00-11-22-33-44-66".to_string(),
            "192.168.1.1; rm -rf /".to_string(),
            "00:11:22:33:44:GG".to_string(),
            "invalid_host".to_string(),
            "  ".to_string(),
        ];

        let serialized = serialize_zapret_conf(&cfg, std::path::Path::new("/opt/etc/xkeen-route/config.json"));
        assert!(serialized.contains("EXCLUDED_IPS=\"192.168.1.50 10.0.0.0/24\""));
        assert!(serialized.contains("EXCLUDED_MACS=\"00:11:22:33:44:55 00-11-22-33-44-66\""));
        assert!(!serialized.contains("rm -rf"));
        assert!(!serialized.contains("invalid_host"));
        assert!(!serialized.contains("GG"));
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
    fn regression_bug_35b_single_zapret_conf_serializer() {
        let dir = std::env::temp_dir().join(format!("xkeen_bug35_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let cfg_path = dir.join("config.json");
        let mut app_cfg = crate::config::AppConfig::default();
        app_cfg.adguard.enabled = true;
        std::fs::write(&cfg_path, serde_json::to_string_pretty(&app_cfg).unwrap())
            .expect("save temp config");

        let serialized = serialize_zapret_conf(&app_cfg.zapret, &cfg_path);
        assert!(
            serialized.contains("ADGUARD_ENABLED=\"1\""),
            "serialize_zapret_conf must read adguard.enabled from passed config_path, got:\n{serialized}"
        );

        let _ = std::fs::remove_dir_all(&dir);
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
    fn regression_bug_37_captive_portal_200_is_not_internet() {
        assert!(!is_failsafe_http_status_valid(200), "HTTP 200 can be a captive portal");
        assert!(is_failsafe_http_status_valid(204));
        assert!(!has_lan_bridge_interface(" eth0: 1 2 br 3\n"));
    }

    #[test]
    fn regression_bug_37_failsafe_rejects_3xx_redirects_and_strict_bridge_check() {
        // Баг 37: Ответ 3xx (captive portal) засчитывался как «интернет есть»,
        // а проверка LAN contains("br") проходила всегда из-за подстроки "bytes" в заголовке /proc/net/dev.

        // 1. Старый код засчитывал (status >= 200 && status < 400), т.е. 3xx считались успехом.
        // Только 204 подтверждает ответ generate_204; 200 может быть captive portal.
        assert!(!is_failsafe_http_status_valid(302), "HTTP 302 captive portal must NOT be valid internet");
        assert!(!is_failsafe_http_status_valid(307), "HTTP 307 redirect must NOT be valid internet");
        assert!(!is_failsafe_http_status_valid(301), "HTTP 301 must NOT be valid internet");
        assert!(is_failsafe_http_status_valid(204), "HTTP 204 No Content is valid");
        assert!(!is_failsafe_http_status_valid(200), "HTTP 200 may be a captive portal");

        // 2. Старый код делал contains("br") на весь /proc/net/dev, что давало true на заголовке "bytes".
        let fake_dev_no_bridge = "Inter-|   Receive                                                |  Transmit\n\
                                  face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed\n\
                                  eth0: 1234567       0    0    0    0     0          0         0  1234567       0    0    0    0     0       0          0\n";
        assert!(
            !has_lan_bridge_interface(fake_dev_no_bridge),
            "dev content with only eth0 must NOT match bridge just because header has 'bytes'"
        );

        let fake_dev_with_bridge = "Inter-|   Receive\n\
                                    face |bytes\n\
                                    br0: 1234567 0 0 0 0 0 0 0 1234567 0 0 0 0 0 0 0\n";
        assert!(
            has_lan_bridge_interface(fake_dev_with_bridge),
            "br0: interface must be detected as LAN bridge"
        );
    }
    #[test]
    fn regression_bug_36_nfqws_cmdline_identifies_our_process() {
        // Баг 36: Любой процесс nfqws в системе считался «нашим» (например сторонний / ручной).
        // Нашим должен считаться только процесс с нашими аргументами (--qnum=200 или xkeen/zapret пути).
        assert!(is_zapret_cmdline_ours("nfqws --daemon --qnum=200 --filter-tcp=80,443"));
        assert!(is_zapret_cmdline_ours("/opt/sbin/nfqws2 --qnum=200 --dpi-desync=fake"));
        assert!(is_zapret_cmdline_ours("nfqws --hostlist=/opt/etc/zapret/zapret-hosts.txt"));
        assert!(!is_zapret_cmdline_ours("nfqws --daemon --qnum=100"), "сторонний nfqws с qnum 100 не наш");
        assert!(!is_zapret_cmdline_ours("/usr/bin/nfqws --dpi-desync=split2"), "сторонний nfqws без наших параметров не наш");
        assert!(!is_zapret_cmdline_ours("python3 test_nfqws.py"), "произвольный процесс не наш");
        assert_eq!(select_our_zapret_pid_from_list("101 202 303", |pid| pid == 202), Some(202));
        assert_eq!(select_our_zapret_pid_from_list("101 202", |_| false), None);
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

    #[test]
    fn test_validate_dns_response_regression() {
        let qid = (0x7A, 0x51);

        // 1. Слишком короткий пакет (< 12 байт)
        assert!(!validate_dns_response(qid, &[0x7A, 0x51, 0x81, 0x80]));
        assert!(!validate_dns_response(qid, &[]));

        // 2. Несовпадение Transaction ID
        let wrong_id_packet = [
            0x12, 0x34, 0x81, 0x80, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
        ];
        assert!(!validate_dns_response(qid, &wrong_id_packet));

        // 3. Пакет с верным ID, но это запрос (QR=0), а не ответ
        let query_packet = [
            0x7A, 0x51, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        assert!(!validate_dns_response(qid, &query_packet));

        // 4. Корректный ответ (QR=1, флаги 0x8180: Standard query response, No error)
        let valid_response = [
            0x7A, 0x51, 0x81, 0x80, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
            0x09, b'l', b'o', b'c', b'a', b'l', b'h', b'o', b's', b't', 0x00,
            0x00, 0x01, 0x00, 0x01,
        ];
        assert!(validate_dns_response(qid, &valid_response));
    }

    #[test]
    fn test_dns_redirect_remove_cmd_regression() {
        // Баг 31: команда удаления редиректа содержит адресное удаление для br+ и Bridge+
        // строго по комментарию xkeen-route-zapret, без слепого удаления чужих правил
        let cmd = super::build_dns_redirect_remove_cmd(crate::routing::DEFAULT_MIHOMO_DNS_PORT);
        assert!(cmd.contains("1053"));
        assert!(cmd.contains("br+"));
        assert!(cmd.contains("Bridge+"));
        assert!(cmd.contains("xkeen-route-zapret"));
        assert!(cmd.contains("ip6tables"));
    }

    #[test]
    fn regression_bug_31b_zapret_dns_redirect_only_own_rules() {
        // Подпункт 31.1 & 31.3:
        // build_dns_redirect_remove_cmd(port) должен собирать команду удаления только правил с маркером xkeen-route-zapret,
        // не содержать слепого grep 'to-ports ...', и подставлять переданный порт (например 7853).
        let cmd = super::build_dns_redirect_remove_cmd(7853);
        assert!(cmd.contains("xkeen-route-zapret"), "команда удаления обязана фильтровать по xkeen-route-zapret");
        assert!(!cmd.contains("grep 'to-ports"), "не должно быть слепого grep 'to-ports без привязки к xkeen-route-zapret");
        assert!(cmd.contains("7853"), "порт должен быть подставлен из параметра");
        assert!(!cmd.contains("1053"), "при порте 7853 не должно оставаться захардкоженного 1053");

        // Подпункт 31.2:
        // zapret_dns_rules_missing(iptables_s_output, port) -> bool
        // Проверяет наличие правил DNS-редиректа Zapret (-m comment --comment "xkeen-route-zapret" ... REDIRECT --to-ports <port>)
        let rules_present = r#"
-A PREROUTING -i br+ -p udp -m comment --comment "xkeen-route-zapret" -j REDIRECT --to-ports 1053
-A PREROUTING -i br+ -p tcp -m comment --comment "xkeen-route-zapret" -j REDIRECT --to-ports 1053
"#;
        assert!(!super::zapret_dns_rules_missing(rules_present, 1053), "правила на месте — missing == false");
        assert!(super::zapret_dns_rules_missing(rules_present, 7853), "правила на 1053, а ищем 7853 — missing == true");

        let foreign_rules = r#"
-A PREROUTING -i br+ -p udp -j REDIRECT --to-ports 1053
-A PREROUTING -i br+ -p tcp -j REDIRECT --to-ports 1053
"#;
        assert!(super::zapret_dns_rules_missing(foreign_rules, 1053), "чужие правила без комментария — missing == true");

        let empty_output = "";
        assert!(super::zapret_dns_rules_missing(empty_output, 1053), "пустой вывод iptables -S — missing == true");

        // Подпункт 31.3: S51zapret и проверка остаточных правил в watchdog используют переданный порт (7853), не 1053
        let s51_7853 = crate::api::render_s51_zapret_script_with_dns_port(7853);
        assert!(s51_7853.contains("--to-ports 7853"), "S51zapret должен редиректить на порт 7853");
        assert!(s51_7853.contains(":1EAD"), "check_dns_resolver_ready в S51zapret должен проверять hex-порт 7853 (0x1EAD)");
        assert!(!s51_7853.contains("--to-ports 1053"), "S51zapret для 7853 не должен содержать --to-ports 1053");
        let left_cmd = super::build_zapret_iptables_left_check_cmd(7853);
        assert!(left_cmd.contains("xkeen-route-zapret"));
        assert!(left_cmd.contains("to-ports 7853"));
        assert!(!left_cmd.contains("1053"));
    }

    #[test]
    fn regression_bug_36b_shell_is_zapret_pid_checks_cmdline() {
        let fn_def = build_zapret_pid_check_fn();
        assert!(fn_def.contains("/proc/$p/cmdline"));
        assert!(!fn_def.contains("case \"$comm\" in\n      nfqws|nfqws2|tpws) return 0 ;;"));
        assert!(!fn_def.contains("case \"$comm\" in\r\n      nfqws|nfqws2|tpws) return 0 ;;"));
        assert!(!fn_def.contains("*nfqws*|*nfqws2*|*tpws*) return 0 ;;"));
        assert!(fn_def.contains("--qnum=200"));
        assert!(fn_def.contains("/opt/etc/zapret"));
        assert!(fn_def.contains("/opt/zapret"));
        assert!(fn_def.contains("xkeen-route"));
    }
}

