use std::sync::atomic::{AtomicBool, Ordering};

pub const DIRECT_CHAIN: &str = "XKEEN_DIRECT";
pub static DIRECT_IPTABLES_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Инициализация цепочки прямого трафика в iptables (таблица filter, цепочка FORWARD).
/// Создает выделенную цепочку XKEEN_DIRECT с двумя неблокирующими правилами-счётчиками:
/// 1. ! -i br+ -o br+ -j RETURN (Входящий прямой трафик WAN -> LAN: Direct Down)
/// 2. -i br+ ! -o br+ -j RETURN (Исходящий прямой трафик LAN -> WAN: Direct Up)
pub fn init_direct_iptables() -> bool {
    #[cfg(target_os = "linux")]
    {
        // 1. Создаем цепочку XKEEN_DIRECT, если её нет
        let _ = std::process::Command::new("iptables")
            .args(["-t", "filter", "-N", DIRECT_CHAIN])
            .output();

        // 2. Очищаем цепочку
        let _ = std::process::Command::new("iptables")
            .args(["-t", "filter", "-F", DIRECT_CHAIN])
            .output();

        // 3. Правило 1: Входящий прямой трафик (! -i br+ -o br+)
        let r1 = std::process::Command::new("iptables")
            .args(["-t", "filter", "-A", DIRECT_CHAIN, "!", "-i", "br+", "-o", "br+", "-m", "comment", "--comment", "xkeen_direct_in", "-j", "RETURN"])
            .output();

        // 4. Правило 2: Исходящий прямой трафик (-i br+ ! -o br+)
        let r2 = std::process::Command::new("iptables")
            .args(["-t", "filter", "-A", DIRECT_CHAIN, "-i", "br+", "!", "-o", "br+", "-m", "comment", "--comment", "xkeen_direct_out", "-j", "RETURN"])
            .output();

        if r1.as_ref().map_or(false, |o| o.status.success()) && r2.as_ref().map_or(false, |o| o.status.success()) {
            // 5. Проверяем наличие перехода из FORWARD
            let check = std::process::Command::new("iptables")
                .args(["-t", "filter", "-C", "FORWARD", "-j", DIRECT_CHAIN])
                .output();

            if !check.map_or(false, |o| o.status.success()) {
                // Вставляем на первое место в FORWARD
                let _ = std::process::Command::new("iptables")
                    .args(["-t", "filter", "-I", "FORWARD", "1", "-j", DIRECT_CHAIN])
                    .output();
            }

            DIRECT_IPTABLES_ACTIVE.store(true, Ordering::Release);
            crate::log_i!("[TRAFFIC] ✓ Аппаратный учёт прямого трафика Netfilter/iptables активирован (цепочка XKEEN_DIRECT)");
            return true;
        }

        DIRECT_IPTABLES_ACTIVE.store(false, Ordering::Release);
        false
    }
    #[cfg(not(target_os = "linux"))]
    {
        DIRECT_IPTABLES_ACTIVE.store(false, Ordering::Release);
        false
    }
}

/// Безопасное снятие цепочки XKEEN_DIRECT из iptables
pub fn cleanup_direct_iptables() {
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("iptables")
            .args(["-t", "filter", "-D", "FORWARD", "-j", DIRECT_CHAIN])
            .output();
        let _ = std::process::Command::new("iptables")
            .args(["-t", "filter", "-F", DIRECT_CHAIN])
            .output();
        let _ = std::process::Command::new("iptables")
            .args(["-t", "filter", "-X", DIRECT_CHAIN])
            .output();
        DIRECT_IPTABLES_ACTIVE.store(false, Ordering::Release);
        crate::log_i!("[TRAFFIC] Цепочка XKEEN_DIRECT в iptables снята");
    }
}

/// Парсинг точных байт из вывода `iptables -t filter -L XKEEN_DIRECT -v -n -x`
pub fn parse_iptables_direct_bytes(output: &str) -> Option<(u64, u64)> {
    let mut direct_down = None;
    let mut direct_up = None;

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Chain") || trimmed.starts_with("pkts") || trimmed.is_empty() {
            continue;
        }

        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() < 7 {
            continue;
        }

        // parts[1] — байты (флаг -x гарантирует точное число без суффиксов K/M/G)
        let Ok(bytes) = parts[1].parse::<u64>() else {
            continue;
        };

        let in_iface = parts[5];
        let out_iface = parts[6];

        if line.contains("xkeen_direct_in") || (in_iface.starts_with('!') && !out_iface.starts_with('!')) {
            direct_down = Some(bytes);
        } else if line.contains("xkeen_direct_out") || (!in_iface.starts_with('!') && out_iface.starts_with('!')) {
            direct_up = Some(bytes);
        }
    }

    match (direct_down, direct_up) {
        (Some(down), Some(up)) => Some((down, up)),
        _ => None,
    }
}

/// Чтение актуальных счетчиков байт прямого трафика из ядра Linux
pub fn read_direct_bytes() -> Option<(u64, u64)> {
    #[cfg(target_os = "linux")]
    {
        let out = std::process::Command::new("iptables")
            .args(["-t", "filter", "-L", DIRECT_CHAIN, "-v", "-n", "-x"])
            .output()
            .ok()?;

        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout);
            return parse_iptables_direct_bytes(&s);
        }

        // Если цепочка слетела (например, сброс правил Netfilter в KeeneticOS ndm),
        // автоматически восстанавливаем правила на лету и повторяем чтение
        if init_direct_iptables() {
            if let Ok(retry_out) = std::process::Command::new("iptables")
                .args(["-t", "filter", "-L", DIRECT_CHAIN, "-v", "-n", "-x"])
                .output()
            {
                if retry_out.status.success() {
                    let s = String::from_utf8_lossy(&retry_out.stdout);
                    return parse_iptables_direct_bytes(&s);
                }
            }
        }

        None
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

/// Проверка фактического присутствия цепочки XKEEN_DIRECT в FORWARD
pub fn is_direct_iptables_active() -> bool {
    #[cfg(target_os = "linux")]
    {
        let check = std::process::Command::new("iptables")
            .args(["-t", "filter", "-C", "FORWARD", "-j", DIRECT_CHAIN])
            .output();
        check.map_or(false, |o| o.status.success())
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_iptables_direct_bytes_success() {
        let sample = "\
Chain XKEEN_DIRECT (1 references)
    pkts      bytes target     prot opt in     out     source               destination         
    1240    1485900 RETURN     all  --  !br+   br+     0.0.0.0/0            0.0.0.0/0            /* xkeen_direct_in */
     850     320450 RETURN     all  --  br+    !br+    0.0.0.0/0            0.0.0.0/0            /* xkeen_direct_out */
";
        let res = parse_iptables_direct_bytes(sample);
        assert_eq!(res, Some((1485900, 320450)));
    }

    #[test]
    fn test_parse_iptables_direct_bytes_empty_chain() {
        let sample = "\
Chain XKEEN_DIRECT (1 references)
    pkts      bytes target     prot opt in     out     source               destination         
";
        let res = parse_iptables_direct_bytes(sample);
        assert_eq!(res, None);
    }

    #[test]
    fn test_parse_iptables_direct_bytes_without_comments() {
        let sample = "\
Chain XKEEN_DIRECT (1 references)
    pkts      bytes target     prot opt in     out     source               destination         
       3      12000 RETURN     all  --  !br+   br+     0.0.0.0/0            0.0.0.0/0           
       5      24000 RETURN     all  --  br+    !br+    0.0.0.0/0            0.0.0.0/0           
";
        let res = parse_iptables_direct_bytes(sample);
        assert_eq!(res, Some((12000, 24000)));
    }
}
