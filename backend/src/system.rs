use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProcessInfo {
    pub pid: u32,
    pub ppid: u32,
    pub name: String,
    pub cmdline: String,
    pub user: String,
    pub state: String,
    pub cpu_percent: f64,
    pub mem_percent: f64,
    pub mem_rss_kb: u64,
    pub threads: u32,
    pub category: String, // "xkeen", "zapret", "keenetic", "system"
    pub protected: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CpuCoreUsage {
    pub core_id: usize,
    pub usage_percent: f64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SystemResources {
    pub cpu_total_percent: f64,
    pub cpu_cores: Vec<CpuCoreUsage>,
    pub memory_total_kb: u64,
    pub memory_used_kb: u64,
    pub memory_free_kb: u64,
    pub memory_buffers_kb: u64,
    pub memory_cached_kb: u64,
    pub memory_available_kb: u64,
    pub memory_percent: f64,
    pub swap_total_kb: u64,
    pub swap_used_kb: u64,
    pub load_avg_1m: f64,
    pub load_avg_5m: f64,
    pub load_avg_15m: f64,
    pub uptime_seconds: u64,
    pub tasks_total: usize,
    pub tasks_running: usize,
    pub tasks_sleeping: usize,
    pub tasks_stopped: usize,
    pub tasks_zombie: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TaskManagerSnapshot {
    pub resources: SystemResources,
    pub processes: Vec<ProcessInfo>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProcessMemoryInfo {
    pub name: String,
    pub display_name: String,
    pub pid: Option<u32>,
    pub rss_bytes: u64,
    pub rss_mb: f64,
    pub running: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StackMemoryInfo {
    pub total_stack_rss_mb: f64,
    pub keenetic_free_mb: f64,
    pub keenetic_total_mb: f64,
    pub keenetic_available_mb: f64,
    pub processes: Vec<ProcessMemoryInfo>,
}

#[derive(Debug, serde::Deserialize)]
pub struct KillRequest {
    pub pid: u32,
    #[serde(default)]
    pub signal: Option<String>,
}

struct CpuSample {
    timestamp: Instant,
    total_cpu_time: u64,
    work_cpu_time: u64,
    core_cpu_times: Vec<(u64, u64)>, // (work, total)
    proc_times: HashMap<u32, u64>,   // pid -> utime + stime
}

static TRACKER: Mutex<Option<CpuSample>> = Mutex::new(None);

pub fn detect_category(name: &str, cmdline: &str) -> &'static str {
    let name_lower = name.to_lowercase();
    let cmd_lower = cmdline.to_lowercase();
    if name_lower == "mihomo"
        || name_lower == "xkeen-route"
        || cmd_lower.contains("mihomo")
        || cmd_lower.contains("xkeen-route")
    {
        "xkeen"
    } else if name_lower == "nfqws"
        || name_lower == "nfqws2"
        || name_lower == "tpws"
        || cmd_lower.contains("nfqws")
        || cmd_lower.contains("nfqws2")
        || cmd_lower.contains("tpws")
    {
        "zapret"
    } else if name_lower == "ndm"
        || name_lower == "ndns"
        || name_lower == "dropbear"
        || name_lower == "hostapd"
        || name_lower == "wpa_supplicant"
        || name_lower == "dnsmasq"
        || name_lower.starts_with("ndm")
        || cmd_lower.contains("/opt/sbin/ndm")
        || cmd_lower.contains("/usr/sbin/ndm")
    {
        "keenetic"
    } else {
        "system"
    }
}

pub const PROTECTED_PROCESSES: &[&str] = &[
    "ndm", "ndns", "ndnproxy", "dnsmasq", "hostapd", "wpa_supplicant",
    "dropbear", "sshd", "udhcpc", "pppd", "netifd",
    "init", "systemd", "procd", "kthreadd", "xkeen-route",
    "cron", "crond", "mihomo", "transmissiond", "nfqws", "nfqws2",
];

pub fn is_protected(pid: u32, name: &str) -> bool {
    if pid <= 2 || pid == std::process::id() {
        return true;
    }
    let n = name.to_lowercase();
    PROTECTED_PROCESSES.iter().any(|&p| n == p || n.starts_with(p))
}

/// Получение полного снимка диспетчера задач (ресурсы + процессы)
pub async fn get_task_manager_snapshot() -> TaskManagerSnapshot {
    #[cfg(target_os = "linux")]
    {
        tokio::task::spawn_blocking(collect_linux_snapshot)
            .await
            .unwrap_or_else(|_| mock_snapshot())
    }
    #[cfg(not(target_os = "linux"))]
    {
        mock_snapshot()
    }
}

#[cfg(target_os = "linux")]
fn collect_linux_snapshot() -> TaskManagerSnapshot {
    let now = Instant::now();

    // 1. /proc/stat -> CPU общий и per-core
    let mut current_total_work = 0u64;
    let mut current_total_all = 0u64;
    let mut current_cores: Vec<(u64, u64)> = Vec::new();

    if let Ok(stat_content) = std::fs::read_to_string("/proc/stat") {
        for line in stat_content.lines() {
            if line.starts_with("cpu ") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 5 {
                    let user: u64 = parts[1].parse().unwrap_or(0);
                    let nice: u64 = parts[2].parse().unwrap_or(0);
                    let system: u64 = parts[3].parse().unwrap_or(0);
                    let idle: u64 = parts[4].parse().unwrap_or(0);
                    let iowait: u64 = parts.get(5).and_then(|s| s.parse().ok()).unwrap_or(0);
                    let irq: u64 = parts.get(6).and_then(|s| s.parse().ok()).unwrap_or(0);
                    let softirq: u64 = parts.get(7).and_then(|s| s.parse().ok()).unwrap_or(0);
                    let steal: u64 = parts.get(8).and_then(|s| s.parse().ok()).unwrap_or(0);

                    let work = user + nice + system + irq + softirq + steal;
                    let total = work + idle + iowait;
                    current_total_work = work;
                    current_total_all = total;
                }
            } else if line.starts_with("cpu") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 5 && parts[0].chars().skip(3).all(|c| c.is_ascii_digit()) {
                    let user: u64 = parts[1].parse().unwrap_or(0);
                    let nice: u64 = parts[2].parse().unwrap_or(0);
                    let system: u64 = parts[3].parse().unwrap_or(0);
                    let idle: u64 = parts[4].parse().unwrap_or(0);
                    let iowait: u64 = parts.get(5).and_then(|s| s.parse().ok()).unwrap_or(0);
                    let irq: u64 = parts.get(6).and_then(|s| s.parse().ok()).unwrap_or(0);
                    let softirq: u64 = parts.get(7).and_then(|s| s.parse().ok()).unwrap_or(0);
                    let steal: u64 = parts.get(8).and_then(|s| s.parse().ok()).unwrap_or(0);

                    let work = user + nice + system + irq + softirq + steal;
                    let total = work + idle + iowait;
                    current_cores.push((work, total));
                }
            }
        }
    }

    // 2. /proc/meminfo
    let mut mem_total_kb = 0u64;
    let mut mem_free_kb = 0u64;
    let mut mem_available_kb = 0u64;
    let mut mem_buffers_kb = 0u64;
    let mut mem_cached_kb = 0u64;
    let mut swap_total_kb = 0u64;
    let mut swap_free_kb = 0u64;

    if let Ok(meminfo) = std::fs::read_to_string("/proc/meminfo") {
        for line in meminfo.lines() {
            if let Some((k, v)) = line.split_once(':') {
                let num: u64 = v
                    .trim()
                    .split_whitespace()
                    .next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                match k.trim() {
                    "MemTotal" => mem_total_kb = num,
                    "MemFree" => mem_free_kb = num,
                    "MemAvailable" => mem_available_kb = num,
                    "Buffers" => mem_buffers_kb = num,
                    "Cached" => mem_cached_kb = num,
                    "SwapTotal" => swap_total_kb = num,
                    "SwapFree" => swap_free_kb = num,
                    _ => {}
                }
            }
        }
    }

    let mem_used_kb = if mem_available_kb > 0 && mem_available_kb <= mem_total_kb {
        mem_total_kb - mem_available_kb
    } else if mem_total_kb >= (mem_free_kb + mem_buffers_kb + mem_cached_kb) {
        mem_total_kb - mem_free_kb - mem_buffers_kb - mem_cached_kb
    } else {
        mem_total_kb.saturating_sub(mem_free_kb)
    };
    let swap_used_kb = swap_total_kb.saturating_sub(swap_free_kb);
    let mem_percent = if mem_total_kb > 0 {
        ((mem_used_kb as f64 / mem_total_kb as f64) * 1000.0).round() / 10.0
    } else {
        0.0
    };

    // 3. /proc/loadavg
    let mut load_1m = 0.0;
    let mut load_5m = 0.0;
    let mut load_15m = 0.0;
    if let Ok(loadavg) = std::fs::read_to_string("/proc/loadavg") {
        let parts: Vec<&str> = loadavg.split_whitespace().collect();
        if parts.len() >= 3 {
            load_1m = parts[0].parse().unwrap_or(0.0);
            load_5m = parts[1].parse().unwrap_or(0.0);
            load_15m = parts[2].parse().unwrap_or(0.0);
        }
    }

    // 4. /proc/uptime
    let mut uptime_secs = 0u64;
    if let Ok(uptime) = std::fs::read_to_string("/proc/uptime") {
        if let Some(first) = uptime.split_whitespace().next() {
            uptime_secs = first.parse::<f64>().map(|v| v as u64).unwrap_or(0);
        }
    }

    // 5. Чтение /proc/[pid]
    let mut raw_procs = Vec::new();
    let mut current_proc_times: HashMap<u32, u64> = HashMap::new();
    let mut tasks_running = 0;
    let mut tasks_sleeping = 0;
    let mut tasks_stopped = 0;
    let mut tasks_zombie = 0;

    if let Ok(entries) = std::fs::read_dir("/proc") {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let file_name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n,
                None => continue,
            };
            let pid: u32 = match file_name.parse() {
                Ok(p) => p,
                Err(_) => continue,
            };

            // Читаем /proc/[pid]/stat
            let stat_path = path.join("stat");
            let stat_content = match std::fs::read_to_string(&stat_path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let rparen = match stat_content.rfind(')') {
                Some(p) => p,
                None => continue,
            };
            let lparen = match stat_content.find('(') {
                Some(p) => p,
                None => continue,
            };

            let comm = &stat_content[lparen + 1..rparen];
            let rest = &stat_content[rparen + 1..];
            let fields: Vec<&str> = rest.split_whitespace().collect();
            if fields.len() < 18 {
                continue;
            }

            let state_str = fields[0].to_string();
            match state_str.as_str() {
                "R" => tasks_running += 1,
                "S" | "D" | "I" => tasks_sleeping += 1,
                "T" | "t" => tasks_stopped += 1,
                "Z" => tasks_zombie += 1,
                _ => tasks_sleeping += 1,
            }

            let ppid: u32 = fields[1].parse().unwrap_or(0);
            let utime: u64 = fields[11].parse().unwrap_or(0);
            let stime: u64 = fields[12].parse().unwrap_or(0);
            let threads: u32 = fields[17].parse().unwrap_or(1);
            let proc_ticks = utime + stime;
            current_proc_times.insert(pid, proc_ticks);

            // Читаем cmdline
            let cmdline_path = path.join("cmdline");
            let cmdline = match std::fs::read(&cmdline_path) {
                Ok(bytes) => {
                    let clean = bytes
                        .into_iter()
                        .map(|b| if b == 0 { b' ' } else { b })
                        .collect::<Vec<u8>>();
                    String::from_utf8_lossy(&clean).trim().to_string()
                }
                Err(_) => String::new(),
            };
            let display_cmd = if cmdline.is_empty() {
                format!("[{}]", comm)
            } else {
                cmdline
            };

            // Читаем /proc/[pid]/status для RSS и Uid
            let mut rss_kb = 0u64;
            let mut user = "root".to_string();
            let status_path = path.join("status");
            if let Ok(status) = std::fs::read_to_string(&status_path) {
                for line in status.lines() {
                    if line.starts_with("VmRSS:") {
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() >= 2 {
                            rss_kb = parts[1].parse().unwrap_or(0);
                        }
                    } else if line.starts_with("Uid:") {
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() >= 2 {
                            if let Ok(uid) = parts[1].parse::<u32>() {
                                user = match uid {
                                    0 => "root".to_string(),
                                    65534 => "nobody".to_string(),
                                    _ => format!("{}", uid),
                                };
                            }
                        }
                    }
                }
            }

            let category = detect_category(comm, &display_cmd).to_string();
            let protected = is_protected(pid, comm);

            raw_procs.push((
                pid,
                ppid,
                comm.to_string(),
                display_cmd,
                user,
                state_str,
                rss_kb,
                threads,
                category,
                protected,
                proc_ticks,
            ));
        }
    }

    // 6. Вычисление дельт CPU между текущим и прошлым сэмплом
    let mut cpu_total_percent = 0.0;
    let mut core_usages: Vec<CpuCoreUsage> = Vec::new();
    let num_cores = current_cores.len().max(1) as f64;

    let mut guard = TRACKER.lock().unwrap_or_else(|e| e.into_inner());
    let mut proc_cpu_map: HashMap<u32, f64> = HashMap::new();

    if let Some(prev) = guard.as_ref() {
        let elapsed = now.duration_since(prev.timestamp).as_secs_f64();
        if elapsed >= 0.2 {
            let delta_total = current_total_all.saturating_sub(prev.total_cpu_time);
            let delta_work = current_total_work.saturating_sub(prev.work_cpu_time);

            if delta_total > 0 {
                let p = (delta_work as f64 / delta_total as f64) * 100.0;
                cpu_total_percent = (p * 10.0).round() / 10.0;
            }

            // Per-core
            for (idx, &(c_work, c_total)) in current_cores.iter().enumerate() {
                let (prev_work, prev_total) = prev
                    .core_cpu_times
                    .get(idx)
                    .copied()
                    .unwrap_or((c_work, c_total));
                let d_tot = c_total.saturating_sub(prev_total);
                let d_wrk = c_work.saturating_sub(prev_work);
                let pct = if d_tot > 0 {
                    ((d_wrk as f64 / d_tot as f64) * 1000.0).round() / 10.0
                } else {
                    0.0
                };
                core_usages.push(CpuCoreUsage {
                    core_id: idx + 1,
                    usage_percent: pct,
                });
            }

            // Per-process CPU
            if delta_total > 0 {
                for (pid, cur_ticks) in &current_proc_times {
                    let prev_ticks = prev.proc_times.get(pid).copied().unwrap_or(0);
                    let d_proc = cur_ticks.saturating_sub(prev_ticks);
                    if d_proc > 0 {
                        let pct = (d_proc as f64 / delta_total as f64) * 100.0 * num_cores;
                        let rounded = (pct * 10.0).round() / 10.0;
                        proc_cpu_map.insert(*pid, rounded);
                    }
                }
            }
        }
    }

    if core_usages.is_empty() {
        for idx in 0..current_cores.len() {
            core_usages.push(CpuCoreUsage {
                core_id: idx + 1,
                usage_percent: 0.0,
            });
        }
    }

    *guard = Some(CpuSample {
        timestamp: now,
        total_cpu_time: current_total_all,
        work_cpu_time: current_total_work,
        core_cpu_times: current_cores,
        proc_times: current_proc_times,
    });

    let mut processes: Vec<ProcessInfo> = raw_procs
        .into_iter()
        .map(|(pid, ppid, name, cmdline, user, state, rss_kb, threads, category, protected, _)| {
            let cpu_percent = proc_cpu_map.get(&pid).copied().unwrap_or(0.0);
            let mem_percent = if mem_total_kb > 0 {
                ((rss_kb as f64 / mem_total_kb as f64) * 1000.0).round() / 10.0
            } else {
                0.0
            };
            ProcessInfo {
                pid,
                ppid,
                name,
                cmdline,
                user,
                state,
                cpu_percent,
                mem_percent,
                mem_rss_kb: rss_kb,
                threads,
                category,
                protected,
            }
        })
        .collect();

    // Сортировка: сначала по CPU %, затем по RSS
    processes.sort_by(|a, b| {
        b.cpu_percent
            .partial_cmp(&a.cpu_percent)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.mem_rss_kb.cmp(&a.mem_rss_kb))
    });

    let tasks_total = processes.len();

    TaskManagerSnapshot {
        resources: SystemResources {
            cpu_total_percent,
            cpu_cores: core_usages,
            memory_total_kb: mem_total_kb,
            memory_used_kb: mem_used_kb,
            memory_free_kb: mem_free_kb,
            memory_buffers_kb: mem_buffers_kb,
            memory_cached_kb: mem_cached_kb,
            memory_available_kb: mem_available_kb,
            memory_percent: mem_percent,
            swap_total_kb: swap_total_kb,
            swap_used_kb: swap_used_kb,
            load_avg_1m: load_1m,
            load_avg_5m: load_5m,
            load_avg_15m: load_15m,
            uptime_seconds: uptime_secs,
            tasks_total,
            tasks_running,
            tasks_sleeping,
            tasks_stopped,
            tasks_zombie,
        },
        processes,
    }
}

pub fn mock_snapshot() -> TaskManagerSnapshot {
    let procs = vec![
        ProcessInfo {
            pid: 1,
            ppid: 0,
            name: "ndm".to_string(),
            cmdline: "/usr/sbin/ndm".to_string(),
            user: "root".to_string(),
            state: "S".to_string(),
            cpu_percent: 0.8,
            mem_percent: 4.8,
            mem_rss_kb: 24576,
            threads: 14,
            category: "keenetic".to_string(),
            protected: true,
        },
        ProcessInfo {
            pid: 245,
            ppid: 1,
            name: "xkeen-route".to_string(),
            cmdline: "/opt/sbin/xkeen-route --host 0.0.0.0 --port 1001".to_string(),
            user: "root".to_string(),
            state: "S".to_string(),
            cpu_percent: 1.2,
            mem_percent: 1.6,
            mem_rss_kb: 8192,
            threads: 6,
            category: "xkeen".to_string(),
            protected: true,
        },
        ProcessInfo {
            pid: 312,
            ppid: 1,
            name: "mihomo".to_string(),
            cmdline: "/opt/sbin/mihomo -d /opt/etc/mihomo".to_string(),
            user: "root".to_string(),
            state: "S".to_string(),
            cpu_percent: 3.4,
            mem_percent: 34.2,
            mem_rss_kb: 175104,
            threads: 22,
            category: "xkeen".to_string(),
            protected: false,
        },
        ProcessInfo {
            pid: 489,
            ppid: 1,
            name: "nfqws".to_string(),
            cmdline: "/opt/sbin/nfqws --daemon --qnum=200 --filter-tcp=80,443 --hostlist-domains=googlevideo.com,youtube.com --dpi-desync=split2 --dpi-desync-cutoff=d4".to_string(),
            user: "root".to_string(),
            state: "S".to_string(),
            cpu_percent: 0.3,
            mem_percent: 0.8,
            mem_rss_kb: 4096,
            threads: 2,
            category: "zapret".to_string(),
            protected: false,
        },
        ProcessInfo {
            pid: 520,
            ppid: 1,
            name: "dnsmasq".to_string(),
            cmdline: "/usr/sbin/dnsmasq -k --conf-file=/tmp/dnsmasq.conf".to_string(),
            user: "nobody".to_string(),
            state: "S".to_string(),
            cpu_percent: 0.1,
            mem_percent: 0.5,
            mem_rss_kb: 2560,
            threads: 1,
            category: "keenetic".to_string(),
            protected: false,
        },
        ProcessInfo {
            pid: 610,
            ppid: 1,
            name: "dropbear".to_string(),
            cmdline: "/opt/sbin/dropbear -p 222".to_string(),
            user: "root".to_string(),
            state: "S".to_string(),
            cpu_percent: 0.0,
            mem_percent: 0.3,
            mem_rss_kb: 1536,
            threads: 1,
            category: "keenetic".to_string(),
            protected: false,
        },
        ProcessInfo {
            pid: 802,
            ppid: 1,
            name: "cron".to_string(),
            cmdline: "/opt/sbin/cron".to_string(),
            user: "root".to_string(),
            state: "S".to_string(),
            cpu_percent: 0.0,
            mem_percent: 0.2,
            mem_rss_kb: 1024,
            threads: 1,
            category: "system".to_string(),
            protected: false,
        },
    ];

    TaskManagerSnapshot {
        resources: SystemResources {
            cpu_total_percent: 5.8,
            cpu_cores: vec![
                CpuCoreUsage {
                    core_id: 1,
                    usage_percent: 7.2,
                },
                CpuCoreUsage {
                    core_id: 2,
                    usage_percent: 4.4,
                },
            ],
            memory_total_kb: 512000,
            memory_used_kb: 216500,
            memory_free_kb: 45000,
            memory_buffers_kb: 35000,
            memory_cached_kb: 215500,
            memory_available_kb: 295500,
            memory_percent: 42.3,
            swap_total_kb: 0,
            swap_used_kb: 0,
            load_avg_1m: 0.28,
            load_avg_5m: 0.22,
            load_avg_15m: 0.16,
            uptime_seconds: 345600,
            tasks_total: procs.len(),
            tasks_running: 1,
            tasks_sleeping: procs.len() - 1,
            tasks_stopped: 0,
            tasks_zombie: 0,
        },
        processes: procs,
    }
}

pub async fn kill_process_by_pid(pid: u32, signal: Option<&str>) -> Result<String, String> {
    if pid <= 2 {
        return Err("Запрещено: системные процессы ядра/init (PID <= 2) защищены от завершения".into());
    }
    if pid == std::process::id() {
        return Err("Запрещено: нельзя завершить процесс веб-панели".into());
    }

    #[cfg(target_os = "linux")]
    {
        let pid_path = format!("/proc/{}", pid);
        if !std::path::Path::new(&pid_path).exists() {
            return Err(format!("Процесс с PID {} не найден", pid));
        }

        if let Ok(comm) = std::fs::read_to_string(format!("/proc/{}/comm", pid)) {
            let comm_trim = comm.trim().to_lowercase();
            if is_protected(pid, &comm_trim) {
                return Err(format!("Запрещено: процесс '{}' (PID {}) является критически важным компонентом KeeneticOS/сети и защищён от завершения", comm.trim(), pid));
            }
        }

        if let Ok(cmdline_raw) = std::fs::read(format!("/proc/{}/cmdline", pid)) {
            let cmdline = String::from_utf8_lossy(&cmdline_raw).replace('\0', " ").to_lowercase();
            for &p in PROTECTED_PROCESSES {
                if cmdline.contains(&format!("/{p}")) || cmdline.starts_with(p) || cmdline.contains(&format!(" {p}")) {
                    return Err(format!("Запрещено: процесс (PID {}) относится к защищённой службе '{}' и не может быть завершён", pid, p));
                }
            }
        }

        // Проверяем PPID: запрещаем завершать демоны init (PPID <= 1)
        if let Ok(stat) = std::fs::read_to_string(format!("/proc/{}/stat", pid)) {
            if let Some(close_paren) = stat.rfind(')') {
                let rest = stat[close_paren + 1..].trim();
                let fields: Vec<&str> = rest.split_whitespace().collect();
                if let Some(ppid_str) = fields.get(1) {
                    if let Ok(ppid) = ppid_str.parse::<u32>() {
                        if ppid <= 1 && pid != std::process::id() {
                            return Err(format!("Запрещено: процесс PID {} является прямым системным потомком init (PPID {})", pid, ppid));
                        }
                    }
                }
            }
        }

        // FE-04: Поддержка SIGKILL (-9) при явном запросе из UI для зависших процессов
        let sig_flag = match signal {
            Some("KILL") | Some("-9") | Some("9") => "-9",
            _ => "-15",
        };

        let output = tokio::process::Command::new("kill")
            .arg(sig_flag)
            .arg(pid.to_string())
            .output()
            .await
            .map_err(|e| format!("Ошибка вызова kill: {e}"))?;

        if output.status.success() {
            Ok(format!("Сигнал {} успешно отправлен процессу PID {}", sig_flag, pid))
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("Команда kill завершилась с ошибкой: {}", stderr.trim()))
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        let sig = signal.unwrap_or("-15");
        Ok(format!("(Симуляция) Сигнал {sig} успешно отправлен процессу PID {pid}"))
    }
}

/// Проверяет, запущен ли процесс с указанным PID и соответствует ли имя/бинарник ожидаемому (Z-08)
pub fn is_process_running_match(pid: u32, expected_name: &str) -> bool {
    if pid <= 1 {
        return false;
    }
    #[cfg(target_os = "linux")]
    {
        let exp = expected_name.to_lowercase();
        // 1. Проверяем /proc/<pid>/comm
        if let Ok(comm) = std::fs::read_to_string(format!("/proc/{}/comm", pid)) {
            let c = comm.trim().to_lowercase();
            if c == exp || c.starts_with(&exp) {
                return true;
            }
        }
        // 2. Проверяем /proc/<pid>/exe (симлинк на бинарник)
        if let Ok(exe_link) = std::fs::read_link(format!("/proc/{}/exe", pid)) {
            let exe_str = exe_link.to_string_lossy().to_lowercase();
            if exe_str.ends_with(&exp) || exe_str.contains(&format!("/{exp}")) {
                return true;
            }
        }
        // 3. Проверяем /proc/<pid>/cmdline
        if let Ok(cmdline_raw) = std::fs::read(format!("/proc/{}/cmdline", pid)) {
            let cmdline = String::from_utf8_lossy(&cmdline_raw).replace('\0', " ").to_lowercase();
            if cmdline.contains(&exp) {
                return true;
            }
        }
        false
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (pid, expected_name);
        true
    }
}

/// Проверяет, запущен ли в системе процесс с указанным именем (comm, exe или cmdline)
pub fn is_process_running(expected_name: &str) -> bool {
    #[cfg(target_os = "linux")]
    {
        let exp = expected_name.to_lowercase();
        if let Ok(entries) = std::fs::read_dir("/proc") {
            for entry in entries.flatten() {
                if let Ok(file_type) = entry.file_type() {
                    if !file_type.is_dir() {
                        continue;
                    }
                }
                let file_name = entry.file_name();
                let pid_str = file_name.to_string_lossy();
                let pid: u32 = match pid_str.parse() {
                    Ok(p) => p,
                    Err(_) => continue,
                };
                if is_process_running_match(pid, &exp) {
                    return true;
                }
            }
        }
        false
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = expected_name;
        true
    }
}

/// Проверяет совпадение имени процесса с учетом ограничения длины TASK_COMM_LEN в Linux (15 символов)
/// и резервной проверки строки аргументов cmdline.
pub fn matches_target_needle(comm_trim: &str, pid: u32, needle: &str, cmdline: Option<&str>) -> bool {
    let comm_lower = comm_trim.to_lowercase();
    let needle_lower = needle.to_lowercase();
    if comm_lower == needle_lower {
        return true;
    }
    if needle == "xkeen-route" && (comm_lower.contains("xkeen-route") || pid == std::process::id()) {
        return true;
    }
    // Ограничение Linux TASK_COMM_LEN: в /proc/<pid>/comm помещается максимум 15 символов.
    // Например, transmission-daemon (19 символов) усекается ядром Linux до transmission-da.
    if comm_lower.len() >= 14 && needle_lower.starts_with(&comm_lower) {
        return true;
    }
    if let Some(cmd) = cmdline {
        let cmd_lower = cmd.to_lowercase();
        if cmd_lower.contains(&needle_lower) {
            return true;
        }
    }
    false
}

/// Замер физической оперативной памяти (RSS) ключевых процессов стека (Mihomo, XRoute, AdGuard Home, Zapret, SMB, Transmission)
/// и свободной памяти KeeneticOS.
pub fn get_stack_memory_info() -> StackMemoryInfo {
    #[cfg(target_os = "linux")]
    {
        // 1. Читаем /proc/meminfo через read_meminfo() (возвращает значения уже в МБ: ram_total_mb, ram_free_mb, swap_total_mb, swap_free_mb)
        let (ram_total_mb, ram_free_mb, _swap_total_mb, _swap_free_mb) = read_meminfo();
        let keenetic_total_mb = ram_total_mb as f64;
        let keenetic_free_mb = ram_free_mb as f64;
        let keenetic_available_mb = ram_free_mb as f64;

        // 2. Сканируем /proc на целевые процессы
        struct TargetDef {
            needle: &'static str,
            display: &'static str,
            is_optional: bool,
        }

        let targets = [
            TargetDef { needle: "mihomo", display: "Mihomo (Ядро)", is_optional: false },
            TargetDef { needle: "xkeen-route", display: "XKeen Route (Панель)", is_optional: false },
            TargetDef { needle: "AdGuardHome", display: "AdGuard Home", is_optional: false },
            TargetDef { needle: "nfqws2", display: "Zapret (nfqws2)", is_optional: false },
            TargetDef { needle: "nfqws", display: "Zapret (nfqws)", is_optional: false },
            TargetDef { needle: "smbd", display: "Samba (smbd)", is_optional: true },
            TargetDef { needle: "transmission-daemon", display: "Transmission", is_optional: true },
        ];

        let mut found_map: HashMap<&'static str, (u32, u64)> = HashMap::new(); // needle -> (pid, rss_kb)

        if let Ok(entries) = std::fs::read_dir("/proc") {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() { continue; }
                let file_name = match path.file_name().and_then(|n| n.to_str()) {
                    Some(n) => n,
                    None => continue,
                };
                let pid: u32 = match file_name.parse() {
                    Ok(p) => p,
                    Err(_) => continue,
                };

                let comm = std::fs::read_to_string(path.join("comm")).unwrap_or_default();
                let comm_trim = comm.trim();
                let cmdline = std::fs::read(path.join("cmdline"))
                    .ok()
                    .map(|b| String::from_utf8_lossy(&b).replace('\0', " "));

                for t in &targets {
                    if matches_target_needle(comm_trim, pid, t.needle, cmdline.as_deref()) {
                        let mut rss_kb = 0u64;
                        if let Ok(status) = std::fs::read_to_string(path.join("status")) {
                            for line in status.lines() {
                                if line.starts_with("VmRSS:") {
                                    if let Some(val) = line.split_whitespace().nth(1).and_then(|s| s.parse().ok()) {
                                        rss_kb = val;
                                        break;
                                    }
                                }
                            }
                        }
                        found_map.entry(t.needle).and_modify(|(_, existing_rss)| *existing_rss += rss_kb).or_insert((pid, rss_kb));
                    }
                }
            }
        }

        let mut processes = Vec::new();
        let mut total_rss_kb = 0u64;

        // Mihomo
        let (m_pid, m_rss) = found_map.get("mihomo").copied().unwrap_or((0, 0));
        let m_running = m_pid > 0 || m_rss > 0;
        total_rss_kb += m_rss;
        processes.push(ProcessMemoryInfo {
            name: "mihomo".to_string(),
            display_name: "Mihomo (Ядро)".to_string(),
            pid: if m_pid > 0 { Some(m_pid) } else { None },
            rss_bytes: m_rss * 1024,
            rss_mb: ((m_rss as f64) / 1024.0 * 10.0).round() / 10.0,
            running: m_running,
        });

        // XKeen Route
        let (xr_pid, xr_rss) = found_map.get("xkeen-route").copied().unwrap_or((std::process::id(), 0));
        total_rss_kb += xr_rss;
        processes.push(ProcessMemoryInfo {
            name: "xkeen-route".to_string(),
            display_name: "XKeen Route (Панель)".to_string(),
            pid: Some(xr_pid),
            rss_bytes: xr_rss * 1024,
            rss_mb: ((xr_rss as f64) / 1024.0 * 10.0).round() / 10.0,
            running: true,
        });

        // AdGuard Home
        let (agh_pid, agh_rss) = found_map.get("AdGuardHome").copied().unwrap_or((0, 0));
        let agh_running = agh_pid > 0 || agh_rss > 0;
        total_rss_kb += agh_rss;
        processes.push(ProcessMemoryInfo {
            name: "AdGuardHome".to_string(),
            display_name: "AdGuard Home".to_string(),
            pid: if agh_pid > 0 { Some(agh_pid) } else { None },
            rss_bytes: agh_rss * 1024,
            rss_mb: ((agh_rss as f64) / 1024.0 * 10.0).round() / 10.0,
            running: agh_running,
        });

        // Zapret
        let (z_pid, z_rss, z_name, z_display) = if let Some(&(pid, rss)) = found_map.get("nfqws2") {
            (pid, rss, "nfqws2", "Zapret 2 (nfqws2)")
        } else if let Some(&(pid, rss)) = found_map.get("nfqws") {
            (pid, rss, "nfqws", "Zapret (nfqws)")
        } else {
            (0, 0, "nfqws2", "Zapret (DPI)")
        };
        let z_running = z_pid > 0 || z_rss > 0;
        total_rss_kb += z_rss;
        processes.push(ProcessMemoryInfo {
            name: z_name.to_string(),
            display_name: z_display.to_string(),
            pid: if z_pid > 0 { Some(z_pid) } else { None },
            rss_bytes: z_rss * 1024,
            rss_mb: ((z_rss as f64) / 1024.0 * 10.0).round() / 10.0,
            running: z_running,
        });

        // smbd (если обнаружен)
        if let Some(&(smb_pid, smb_rss)) = found_map.get("smbd") {
            total_rss_kb += smb_rss;
            processes.push(ProcessMemoryInfo {
                name: "smbd".to_string(),
                display_name: "Samba (smbd)".to_string(),
                pid: Some(smb_pid),
                rss_bytes: smb_rss * 1024,
                rss_mb: ((smb_rss as f64) / 1024.0 * 10.0).round() / 10.0,
                running: true,
            });
        }

        // transmission-daemon (если обнаружен)
        if let Some(&(tr_pid, tr_rss)) = found_map.get("transmission-daemon") {
            total_rss_kb += tr_rss;
            processes.push(ProcessMemoryInfo {
                name: "transmission-daemon".to_string(),
                display_name: "Transmission".to_string(),
                pid: Some(tr_pid),
                rss_bytes: tr_rss * 1024,
                rss_mb: ((tr_rss as f64) / 1024.0 * 10.0).round() / 10.0,
                running: true,
            });
        }

        let total_stack_rss_mb = ((total_rss_kb as f64) / 1024.0 * 10.0).round() / 10.0;

        StackMemoryInfo {
            total_stack_rss_mb,
            keenetic_free_mb,
            keenetic_total_mb,
            keenetic_available_mb,
            processes,
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        StackMemoryInfo {
            total_stack_rss_mb: 66.5,
            keenetic_free_mb: 284.5,
            keenetic_total_mb: 512.0,
            keenetic_available_mb: 310.0,
            processes: vec![
                ProcessMemoryInfo {
                    name: "mihomo".to_string(),
                    display_name: "Mihomo (Ядро)".to_string(),
                    pid: Some(1042),
                    rss_bytes: 25375744,
                    rss_mb: 24.2,
                    running: true,
                },
                ProcessMemoryInfo {
                    name: "xkeen-route".to_string(),
                    display_name: "XKeen Route (Панель)".to_string(),
                    pid: Some(std::process::id()),
                    rss_bytes: 19398656,
                    rss_mb: 18.5,
                    running: true,
                },
                ProcessMemoryInfo {
                    name: "AdGuardHome".to_string(),
                    display_name: "AdGuard Home".to_string(),
                    pid: Some(2150),
                    rss_bytes: 20761804,
                    rss_mb: 19.8,
                    running: true,
                },
                ProcessMemoryInfo {
                    name: "nfqws2".to_string(),
                    display_name: "Zapret 2 (nfqws2)".to_string(),
                    pid: Some(3021),
                    rss_bytes: 4194304,
                    rss_mb: 4.0,
                    running: true,
                },
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_protected_system_daemons() {
        assert!(is_protected(1, "init"));
        assert!(is_protected(2, "kthreadd"));
        assert!(is_protected(100, "ndm"));
        assert!(is_protected(101, "dnsmasq"));
        assert!(!is_protected(999, "my_custom_script"));
    }

    #[test]
    fn test_is_process_running_match_pid_bounds() {
        assert!(!is_process_running_match(0, "nfqws"));
        assert!(!is_process_running_match(1, "nfqws"));
    }

    #[test]
    fn test_is_process_running_regression() {
        // Гарантирует наличие и работоспособность функции проверки процесса по имени (предотвращает регрессию сборки в Linux CI)
        assert!(is_process_running("AdGuardHome"));
    }

    #[test]
    fn test_stack_memory_info_structure() {
        let mem = get_stack_memory_info();
        assert!(mem.keenetic_total_mb >= 16.0, "Keenetic total RAM must be >= 16 MB, not double-divided (was {})", mem.keenetic_total_mb);
        assert!(mem.keenetic_free_mb >= 1.0, "Keenetic free RAM must be >= 1 MB (was {})", mem.keenetic_free_mb);
        assert!(!mem.processes.is_empty());
        assert!(mem.processes.iter().any(|p| p.name == "mihomo"));
        assert!(mem.processes.iter().any(|p| p.name == "xkeen-route"));
        assert!(mem.processes.iter().any(|p| p.name == "AdGuardHome"));
    }

    #[test]
    fn test_anti_regression_keenetic_ram_not_double_divided() {
        // Защита от багов "0 МБ свободно из 1 МБ":
        // Проверяем, что meminfo для роутера 512 МБ корректно парсится в 512 МБ (а не в 0.5 МБ)
        let sample_512mb = "\
MemTotal:         524288 kB
MemFree:          131072 kB
MemAvailable:     262144 kB
";
        let (total_ram, free_ram, _, _) = parse_meminfo_str(sample_512mb);
        assert_eq!(total_ram, 512, "524288 kB must equal 512 MB");
        assert_eq!(free_ram, 256, "262144 kB must equal 256 MB");
    }

    #[test]
    fn test_matches_target_needle_handles_linux_comm_truncation() {
        // Проверка усечения длинного имени transmission-daemon (19 символов) ядром Linux до 15 символов
        assert!(matches_target_needle("transmission-da", 1234, "transmission-daemon", None));
        assert!(matches_target_needle("mihomo", 100, "mihomo", None));
        assert!(matches_target_needle("AdGuardHome", 200, "AdGuardHome", None));
        assert!(matches_target_needle("nfqws2", 300, "nfqws2", None));
        assert!(matches_target_needle("nfqws", 400, "nfqws", None));
        assert!(matches_target_needle("smbd", 500, "smbd", None));
        assert!(!matches_target_needle("other", 600, "transmission-daemon", None));
        // Проверка через аргументы cmdline
        assert!(matches_target_needle("sh", 700, "transmission-daemon", Some("/usr/bin/transmission-daemon --config-dir /opt/etc")));
    }

    #[test]
    fn test_swap_size_presets_validation() {
        assert!(ALLOWED_SWAP_SIZES_MB.contains(&256));
        assert!(ALLOWED_SWAP_SIZES_MB.contains(&512));
        assert!(ALLOWED_SWAP_SIZES_MB.contains(&1024));
        assert_eq!(ALLOWED_SWAP_SIZES_MB.len(), 3);

        // Invalid sizes must be rejected
        let res_invalid = create_and_enable_swap(128);
        assert!(res_invalid.is_err());
        assert!(res_invalid.err().unwrap().contains("Недопустимый размер Swap: 128 МБ"));

        let res_too_large = create_and_enable_swap(2048);
        assert!(res_too_large.is_err());
    }

    #[test]
    fn test_parse_meminfo_str() {
        let sample = "\
MemTotal:         256000 kB
MemFree:           16000 kB
MemAvailable:      24000 kB
Buffers:            4000 kB
Cached:            12000 kB
SwapTotal:        524288 kB
SwapFree:         500000 kB
";
        let (total_ram, free_ram, total_swap, free_swap) = parse_meminfo_str(sample);
        assert_eq!(total_ram, 250); // 256000 / 1024
        assert_eq!(free_ram, 23);   // 24000 / 1024
        assert_eq!(total_swap, 512); // 524288 / 1024
        assert_eq!(free_swap, 488);  // 500000 / 1024
    }

    #[test]
    fn test_parse_df_output() {
        let df_sample = "\
Filesystem           1K-blocks      Used Available Use% Mounted on
/dev/sda1              3840000    500000   3140000  14% /opt
";
        let (avail_mb, total_mb) = parse_df_output(df_sample).expect("df parse must succeed");
        assert_eq!(avail_mb, 3066); // 3140000 / 1024
        assert_eq!(total_mb, 3750); // 3840000 / 1024
    }

    #[test]
    fn test_swap_init_script_content_sanity() {
        assert!(INIT_SCRIPT_CONTENT.contains("swapon /opt/swapfile"));
        assert!(INIT_SCRIPT_CONTENT.contains("swapoff /opt/swapfile"));
        assert!(INIT_SCRIPT_CONTENT.contains("sysctl -w vm.swappiness=10"));
    }

    #[test]
    fn test_swap_status_serialization_roundtrip() {
        let status = SwapStatus {
            swap_supported: true,
            swap_active: true,
            swap_total_mb: 512,
            swap_used_mb: 12,
            swap_free_mb: 500,
            swap_file_exists: true,
            swap_file_path: SWAP_FILE_PATH.to_string(),
            swappiness: 10,
            opt_disk_free_mb: 2048,
            opt_disk_total_mb: 4096,
            ram_total_mb: 256,
            ram_free_mb: 30,
            recommend_swap: false,
        };
        let json_val = serde_json::to_value(&status).expect("Serialization must succeed");
        assert_eq!(json_val["swap_total_mb"], 512);
        assert_eq!(json_val["swap_file_path"], "/opt/swapfile");
        assert_eq!(json_val["swappiness"], 10);
    }
}

// ============================================================================
// Системная логика «Swap в 1 клик» (Keenetic / Entware)
// ============================================================================

pub const SWAP_FILE_PATH: &str = "/opt/swapfile";
pub const INIT_SCRIPT_PATH: &str = "/opt/etc/init.d/S01swap";
pub const SAFETY_MARGIN_MB: u64 = 300;
pub const TARGET_SWAPPINESS: u32 = 10;
pub const ALLOWED_SWAP_SIZES_MB: [u64; 3] = [256, 512, 1024];

pub const INIT_SCRIPT_CONTENT: &str = r#"#!/bin/sh
case "$1" in
  start)
    [ -f /opt/swapfile ] && swapon /opt/swapfile && sysctl -w vm.swappiness=10 >/dev/null 2>&1
    ;;
  stop)
    swapoff /opt/swapfile 2>/dev/null
    ;;
  *)
    echo "Usage: $0 {start|stop}"
    exit 1
    ;;
esac
exit 0
"#;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct SwapStatus {
    pub swap_supported: bool,
    pub swap_active: bool,
    pub swap_total_mb: u64,
    pub swap_used_mb: u64,
    pub swap_free_mb: u64,
    pub swap_file_exists: bool,
    pub swap_file_path: String,
    pub swappiness: u32,
    pub opt_disk_free_mb: u64,
    pub opt_disk_total_mb: u64,
    pub ram_total_mb: u64,
    pub ram_free_mb: u64,
    pub recommend_swap: bool,
}

/// Читает `/proc/meminfo` и возвращает:
/// `(ram_total_mb, ram_free_mb, swap_total_mb, swap_free_mb)`
pub fn read_meminfo() -> (u64, u64, u64, u64) {
    #[cfg(target_os = "linux")]
    {
        parse_meminfo_str(&std::fs::read_to_string("/proc/meminfo").unwrap_or_default())
    }
    #[cfg(not(target_os = "linux"))]
    {
        (256, 64, 0, 0)
    }
}

pub fn parse_meminfo_str(content: &str) -> (u64, u64, u64, u64) {
    let mut mem_total_kb = 0u64;
    let mut mem_free_kb = 0u64;
    let mut mem_available_kb = None;
    let mut swap_total_kb = 0u64;
    let mut swap_free_kb = 0u64;

    for line in content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }
        let val = parts[1].parse::<u64>().unwrap_or(0);
        match parts[0] {
            "MemTotal:" => mem_total_kb = val,
            "MemFree:" => mem_free_kb = val,
            "MemAvailable:" => mem_available_kb = Some(val),
            "SwapTotal:" => swap_total_kb = val,
            "SwapFree:" => swap_free_kb = val,
            _ => {}
        }
    }

    let effective_free_kb = mem_available_kb.unwrap_or(mem_free_kb);
    (
        mem_total_kb / 1024,
        effective_free_kb / 1024,
        swap_total_kb / 1024,
        swap_free_kb / 1024,
    )
}

/// Проверяет, активен ли `/opt/swapfile` (или иной swap) через `/proc/swaps`
pub fn is_swap_active(swap_total_mb: u64) -> bool {
    #[cfg(target_os = "linux")]
    {
        if let Ok(swaps) = std::fs::read_to_string("/proc/swaps") {
            if swaps.lines().skip(1).any(|line| !line.trim().is_empty()) {
                return true;
            }
        }
        swap_total_mb > 0
    }
    #[cfg(not(target_os = "linux"))]
    {
        swap_total_mb > 0
    }
}

/// Проверяет поддержку swap ядром Linux и наличие примонтированного `/opt`
pub fn is_swap_supported() -> bool {
    #[cfg(target_os = "linux")]
    {
        std::path::Path::new("/proc/swaps").exists() && std::path::Path::new("/opt").is_dir()
    }
    #[cfg(not(target_os = "linux"))]
    {
        true
    }
}

/// Читает текущее значение `vm.swappiness`
pub fn read_swappiness() -> u32 {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string("/proc/sys/vm/swappiness")
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
            .unwrap_or(60)
    }
    #[cfg(not(target_os = "linux"))]
    {
        10
    }
}

/// Парсер строки вывода df -k /opt
pub fn parse_df_output(stdout: &str) -> Result<(u64, u64), String> {
    let data_line = stdout
        .lines()
        .skip(1)
        .last()
        .ok_or_else(|| "Не удалось разобрать вывод df -k /opt".to_string())?;

    let cols: Vec<&str> = data_line.split_whitespace().collect();
    if cols.len() >= 5 {
        let total = cols[cols.len() - 5].parse::<u64>().unwrap_or(0);
        let avail = cols[cols.len() - 3].parse::<u64>().unwrap_or(0);
        Ok((avail / 1024, total / 1024))
    } else {
        Err("Некорректный формат вывода df -k /opt".to_string())
    }
}

/// Возвращает `(opt_disk_free_mb, opt_disk_total_mb)` для раздела `/opt` через `df -k /opt`
pub fn get_opt_disk_space_mb() -> Result<(u64, u64), String> {
    #[cfg(target_os = "linux")]
    {
        if !std::path::Path::new("/opt").exists() {
            return Err("Раздел /opt не найден на устройстве".to_string());
        }

        let output = std::process::Command::new("df")
            .args(["-k", "/opt"])
            .output()
            .map_err(|e| format!("Ошибка вызова df -k /opt: {}", e))?;

        if !output.status.success() {
            return Err(format!(
                "Команда df завершилась с ошибкой: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        parse_df_output(&stdout)
    }
    #[cfg(not(target_os = "linux"))]
    {
        Ok((2048, 4096))
    }
}

/// Собирает полный статус Swap для `GET /api/system/swap/status`
pub fn get_swap_status() -> SwapStatus {
    let (ram_total_mb, ram_free_mb, swap_total_mb, swap_free_mb) = read_meminfo();
    let swap_used_mb = swap_total_mb.saturating_sub(swap_free_mb);
    let swap_active = is_swap_active(swap_total_mb);
    let swap_file_exists = std::path::Path::new(SWAP_FILE_PATH).exists();
    let swap_supported = is_swap_supported();
    let swappiness = read_swappiness();
    let (opt_disk_free_mb, opt_disk_total_mb) = get_opt_disk_space_mb().unwrap_or((0, 0));

    // Рекомендуем включить Swap на роутерах с ОЗУ <= 256 МБ (или при свободной ОЗУ < 32 МБ), если Swap не активен
    let recommend_swap =
        swap_supported && !swap_active && (ram_total_mb <= 256 || ram_free_mb < 32);

    SwapStatus {
        swap_supported,
        swap_active,
        swap_total_mb,
        swap_used_mb,
        swap_free_mb,
        swap_file_exists,
        swap_file_path: SWAP_FILE_PATH.to_string(),
        swappiness,
        opt_disk_free_mb,
        opt_disk_total_mb,
        ram_total_mb,
        ram_free_mb,
        recommend_swap,
    }
}

/// Создаёт и мгновенно активирует файл подкачки `/opt/swapfile` заданного размера (256, 512 или 1024 МБ)
pub fn create_and_enable_swap(size_mb: u64) -> Result<SwapStatus, String> {
    if !ALLOWED_SWAP_SIZES_MB.contains(&size_mb) {
        return Err(format!(
            "Недопустимый размер Swap: {} МБ (допустимые значения: 256, 512, 1024 МБ)",
            size_mb
        ));
    }

    // 1. Проверка свободного места на `/opt` (требуется Размер_Swap + 300 МБ)
    let (free_mb, _total_mb) = get_opt_disk_space_mb()?;
    let existing_swap_mb = std::fs::metadata(SWAP_FILE_PATH)
        .map(|m| m.len() / (1024 * 1024))
        .unwrap_or(0);
    let effective_free_mb = free_mb.saturating_add(existing_swap_mb);
    let required_mb = size_mb.saturating_add(SAFETY_MARGIN_MB);

    if effective_free_mb < required_mb {
        return Err(format!(
            "Недостаточно места на накопителе: свободно {} МБ, требуется {} МБ",
            effective_free_mb, required_mb
        ));
    }

    #[cfg(target_os = "linux")]
    {
        // Если прежний swapfile был активен, отключаем его перед пересозданием
        let _ = std::process::Command::new("swapoff").arg(SWAP_FILE_PATH).output();

        // 2. Безопасное выделение файла без фрагментации («No Holes») через `dd`
        let dd_out = std::process::Command::new("dd")
            .args([
                "if=/dev/zero",
                &format!("of={}", SWAP_FILE_PATH),
                "bs=1M",
                &format!("count={}", size_mb),
            ])
            .output()
            .map_err(|e| format!("Не удалось запустить dd: {}", e))?;

        if !dd_out.status.success() {
            let _ = std::fs::remove_file(SWAP_FILE_PATH);
            return Err(format!(
                "Ошибка при создании файла подкачки (dd): {}",
                String::from_utf8_lossy(&dd_out.stderr).trim()
            ));
        }

        // 3. Строгие права `chmod 600 /opt/swapfile`
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Err(e) = std::fs::set_permissions(SWAP_FILE_PATH, std::fs::Permissions::from_mode(0o600)) {
                let _ = std::fs::remove_file(SWAP_FILE_PATH);
                return Err(format!(
                    "Не удалось установить права 600 на {}: {}",
                    SWAP_FILE_PATH, e
                ));
            }
        }

        // 4. Форматирование `mkswap /opt/swapfile`
        let mkswap_out = std::process::Command::new("mkswap")
            .arg(SWAP_FILE_PATH)
            .output()
            .map_err(|e| format!("Не удалось запустить mkswap: {}", e))?;

        if !mkswap_out.status.success() {
            let _ = std::fs::remove_file(SWAP_FILE_PATH);
            return Err(format!(
                "Ошибка mkswap: {}",
                String::from_utf8_lossy(&mkswap_out.stderr).trim()
            ));
        }

        // 5. Мгновенная активация `swapon /opt/swapfile`
        let swapon_out = std::process::Command::new("swapon")
            .arg(SWAP_FILE_PATH)
            .output()
            .map_err(|e| format!("Не удалось запустить swapon: {}", e))?;

        if !swapon_out.status.success() {
            let _ = std::fs::remove_file(SWAP_FILE_PATH);
            return Err(format!(
                "Ошибка активации swapon: {}",
                String::from_utf8_lossy(&swapon_out.stderr).trim()
            ));
        }

        // 6. Защита от износа флеш-памяти: `sysctl -w vm.swappiness=10`
        let sysctl_out = std::process::Command::new("sysctl")
            .args(["-w", &format!("vm.swappiness={}", TARGET_SWAPPINESS)])
            .output()
            .map_err(|e| format!("Не удалось запустить sysctl: {}", e))?;

        if !sysctl_out.status.success() {
            let _ = std::fs::write("/proc/sys/vm/swappiness", format!("{}\n", TARGET_SWAPPINESS));
        }

        // 7. Создание автозагрузочного скрипта `/opt/etc/init.d/S01swap` с правами `chmod +x`
        if let Some(parent) = std::path::Path::new(INIT_SCRIPT_PATH).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(INIT_SCRIPT_PATH, INIT_SCRIPT_CONTENT)
            .map_err(|e| format!("Не удалось записать {}: {}", INIT_SCRIPT_PATH, e))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(INIT_SCRIPT_PATH, std::fs::Permissions::from_mode(0o755)).map_err(|e| {
                format!(
                    "Не удалось установить права +x на {}: {}",
                    INIT_SCRIPT_PATH, e
                )
            })?;
        }
    }

    Ok(get_swap_status())
}

/// Отключает и полностью удаляет `/opt/swapfile` и `/opt/etc/init.d/S01swap`
pub fn disable_and_remove_swap() -> Result<SwapStatus, String> {
    #[cfg(target_os = "linux")]
    {
        if std::path::Path::new(SWAP_FILE_PATH).exists() {
            let swapoff_out = std::process::Command::new("swapoff")
                .arg(SWAP_FILE_PATH)
                .output()
                .map_err(|e| format!("Не удалось запустить swapoff: {}", e))?;

            if !swapoff_out.status.success() {
                let swaps = std::fs::read_to_string("/proc/swaps").unwrap_or_default();
                if swaps.contains(SWAP_FILE_PATH) {
                    return Err(format!(
                        "Не удалось отключить Swap (возможно, недостаточно свободной ОЗУ для выгрузки страниц): {}",
                        String::from_utf8_lossy(&swapoff_out.stderr).trim()
                    ));
                }
            }

            std::fs::remove_file(SWAP_FILE_PATH)
                .map_err(|e| format!("Не удалось удалить {}: {}", SWAP_FILE_PATH, e))?;
        }

        if std::path::Path::new(INIT_SCRIPT_PATH).exists() {
            std::fs::remove_file(INIT_SCRIPT_PATH)
                .map_err(|e| format!("Не удалось удалить {}: {}", INIT_SCRIPT_PATH, e))?;
        }
    }

    Ok(get_swap_status())
}