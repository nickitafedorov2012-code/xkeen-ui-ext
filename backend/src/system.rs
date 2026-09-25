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
        || name_lower == "tpws"
        || cmd_lower.contains("nfqws")
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

pub fn is_protected(pid: u32, name: &str) -> bool {
    pid <= 1
        || pid == std::process::id()
        || name == "ndm"
        || name == "xkeen-route"
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
    if pid <= 1 {
        return Err("Запрещено: процесс init/ndm (PID 1) является системным и защищён от завершения".into());
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
            let comm_trim = comm.trim();
            if comm_trim == "ndm" || comm_trim == "xkeen-route" {
                return Err(format!("Процесс '{}' (PID {}) защищён от завершения", comm_trim, pid));
            }
        }

        let sig_flag = match signal.unwrap_or("TERM").to_uppercase().as_str() {
            "KILL" | "SIGKILL" | "9" => "-9",
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
        let _ = (pid, signal);
        Ok(format!("(Симуляция) Сигнал успешно отправлен процессу PID {}", pid))
    }
}