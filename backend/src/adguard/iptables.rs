use super::AGH_IPTABLES_CHAIN;
#[cfg(target_os = "linux")]
use super::{AGH_LAN_INTERFACES, DEFAULT_ADGUARD_DNS_FALLBACK_PORT, detect_adguard_dns_port};

/// каждое правило вставляется на нарастающую позицию 1, 2, 3..., а не все на позицию 1.
pub fn reinsert_rules_commands(rules: &[&str], to_port: u16) -> Vec<String> {
    let mut cmds = Vec::with_capacity(rules.len() * 2);
    for (idx, raw_line) in rules.iter().enumerate() {
        let line = raw_line.trim();
        if !line.starts_with("-A ") {
            continue;
        }
        let pos = idx + 1;
        let del_cmd = line.replacen("-A ", "-D ", 1);
        let mut parts = line.splitn(3, ' ');
        let _flag = parts.next();
        let chain = parts.next().unwrap_or("");
        let rest = parts.next().unwrap_or("");
        let updated_rest = if let Some(p_idx) = rest.find("--to-ports ") {
            let after = &rest[p_idx + "--to-ports ".len()..];
            let end = after.find(' ').unwrap_or(after.len());
            format!("{}--to-ports {}{}", &rest[..p_idx], to_port, &after[end..])
        } else {
            rest.to_string()
        };
        let ipt = crate::system::iptables_cmd_str();
        cmds.push(format!("{ipt} -t nat {del_cmd}"));
        cmds.push(format!("{ipt} -t nat -I {chain} {pos} {updated_rest}"));
    }
    cmds
}

pub fn build_sync_tunnel_dns_ports_script(from_ports: &str, to_port: u16) -> String {
    let ipt = crate::system::iptables_cmd_str();
    format!(
        "for chain in XKEEN_GAMING XKEEN_BS; do \\\n\
            pos=1; \\\n\
            {ipt} -t nat -S $chain 2>/dev/null | grep -E 'to-ports ({from_ports})' | while read -r line; do \\\n\
                del_cmd=$(echo \"$line\" | sed 's/^-A/-D/'); \\\n\
                if [ \"$pos\" -eq 1 ]; then \\\n\
                    add_cmd=$(echo \"$line\" | sed -E 's/--to-ports [0-9]+/--to-ports {to_port}/' | sed \"s/^-A $chain/-I $chain 1/\"); \\\n\
                else \\\n\
                    add_cmd=$(echo \"$line\" | sed -E 's/--to-ports [0-9]+/--to-ports {to_port}/' | sed \"s/^-A $chain/-I $chain $pos/\"); \\\n\
                fi; \\\n\
                pos=$((pos + 1)); \\\n\
                {ipt} -t nat $del_cmd 2>/dev/null || true; \\\n\
                {ipt} -t nat $add_cmd 2>/dev/null || true; \\\n\
            done; \\\n\
        done; \\\n\
        for hook in /opt/etc/ndm/netfilter.d/060-xkeen-gaming.sh /opt/etc/ndm/netfilter.d/061-xkeen-bs.sh; do \\\n\
            if [ -f \"$hook\" ]; then \\\n\
                sed -i -E 's/--to-ports ({from_ports})/--to-ports {to_port}/g' \"$hook\" 2>/dev/null || true; \\\n\
            fi; \\\n\
        done",
        ipt = ipt,
        from_ports = from_ports,
        to_port = to_port
    )
}

pub fn build_adguard_ndm_hook_content(dns_port: u16) -> String {
    let port_str = dns_port.to_string();
    let mihomo_port = crate::routing::mihomo_dns_port_from_path(&crate::config::MihomoConfig::default().config_path);
    let sync_script = build_sync_tunnel_dns_ports_script(&format!("{mihomo_port}|5353|5354"), dns_port);
    let ipt = crate::system::iptables_cmd_str();
    format!(
        "#!/bin/sh\n[ \"$table\" != \"nat\" ] && [ \"$table\" != \"filter\" ] && [ -n \"$table\" ] && exit 0\n\
        {ipt} -t nat -N {chain} 2>/dev/null || true\n\
        {ipt} -t nat -F {chain}\n\
        {ipt} -t nat -A {chain} -d 127.0.0.1 -j RETURN\n\
        {ipt} -t nat -A {chain} -p udp --dport 53 -j REDIRECT --to-ports {port}\n\
        {ipt} -t nat -A {chain} -p tcp --dport 53 -j REDIRECT --to-ports {port}\n\
        for iface in br+ Bridge+ ra+; do\n\
            {ipt} -t nat -C PREROUTING -i \"$iface\" -p udp --dport 53 -j {chain} 2>/dev/null || {ipt} -t nat -I PREROUTING 1 -i \"$iface\" -p udp --dport 53 -j {chain}\n\
            {ipt} -t nat -C PREROUTING -i \"$iface\" -p tcp --dport 53 -j {chain} 2>/dev/null || {ipt} -t nat -I PREROUTING 1 -i \"$iface\" -p tcp --dport 53 -j {chain}\n\
            {ipt} -t filter -C INPUT -i \"$iface\" -p udp --dport {port} -j ACCEPT 2>/dev/null || {ipt} -t filter -I INPUT 1 -i \"$iface\" -p udp --dport {port} -j ACCEPT\n\
            {ipt} -t filter -C INPUT -i \"$iface\" -p tcp --dport {port} -j ACCEPT 2>/dev/null || {ipt} -t filter -I INPUT 1 -i \"$iface\" -p tcp --dport {port} -j ACCEPT\n\
        done\n\
        {ipt} -t filter -C INPUT -i lo -p udp --dport {port} -j ACCEPT 2>/dev/null || {ipt} -t filter -I INPUT 1 -i lo -p udp --dport {port} -j ACCEPT\n\
        {ipt} -t filter -C INPUT -i lo -p tcp --dport {port} -j ACCEPT 2>/dev/null || {ipt} -t filter -I INPUT 1 -i lo -p tcp --dport {port} -j ACCEPT\n\
        {sync_script}\n",
        ipt = ipt,
        chain = AGH_IPTABLES_CHAIN,
        port = port_str,
        sync_script = sync_script
    )
}

/// Применение изолированной цепочки XKEEN_ADGUARD в iptables -t nat для перехвата портов 53 UDP/TCP
pub fn apply_adguard_iptables(dns_port: u16) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        // 1. Создаем цепочку XKEEN_ADGUARD, если она не создана, и очищаем ее
        if !crate::system::run_iptables_ok(&["-t", "nat", "-N", AGH_IPTABLES_CHAIN]) {
            // Цепочка могла уже существовать — проверяем через -F ниже
        }
        crate::system::run_iptables(&["-t", "nat", "-F", AGH_IPTABLES_CHAIN])?;

        // 2. Исключаем трафик loopback
        crate::system::run_iptables(&["-t", "nat", "-A", AGH_IPTABLES_CHAIN, "-d", "127.0.0.1", "-j", "RETURN"])?;

        // 3. Редирект входящих DNS-запросов (порт 53) на порт AdGuard Home
        let port_str = dns_port.to_string();
        crate::system::run_iptables(&["-t", "nat", "-A", AGH_IPTABLES_CHAIN, "-p", "udp", "--dport", "53", "-j", "REDIRECT", "--to-ports", &port_str])?;
        crate::system::run_iptables(&["-t", "nat", "-A", AGH_IPTABLES_CHAIN, "-p", "tcp", "--dport", "53", "-j", "REDIRECT", "--to-ports", &port_str])?;

        // 4. Встраиваем переход в XKEEN_ADGUARD в PREROUTING для LAN мостов и Wi-Fi интерфейсов (br+, Bridge+, ra+)
        for iface in AGH_LAN_INTERFACES {
            if !crate::system::run_iptables_ok(&["-t", "nat", "-C", "PREROUTING", "-i", iface, "-p", "udp", "--dport", "53", "-j", AGH_IPTABLES_CHAIN]) {
                crate::system::run_iptables(&["-t", "nat", "-I", "PREROUTING", "1", "-i", iface, "-p", "udp", "--dport", "53", "-j", AGH_IPTABLES_CHAIN])?;
            }
            if !crate::system::run_iptables_ok(&["-t", "nat", "-C", "PREROUTING", "-i", iface, "-p", "tcp", "--dport", "53", "-j", AGH_IPTABLES_CHAIN]) {
                crate::system::run_iptables(&["-t", "nat", "-I", "PREROUTING", "1", "-i", iface, "-p", "tcp", "--dport", "53", "-j", AGH_IPTABLES_CHAIN])?;
            }
        }

        // 5. Разрешаем входящий трафик на порт AdGuard в цепочке INPUT таблицы filter ТОЛЬКО для LAN-интерфейсов и lo (защита от WAN-доступа, баг 44)
        let mut allowed_input_ifaces = AGH_LAN_INTERFACES.to_vec();
        allowed_input_ifaces.push("lo");
        for iface in allowed_input_ifaces {
            for proto in &["udp", "tcp"] {
                if !crate::system::run_iptables_ok(&["-t", "filter", "-C", "INPUT", "-i", iface, "-p", proto, "--dport", &port_str, "-j", "ACCEPT"]) {
                    crate::system::run_iptables(&["-t", "filter", "-I", "INPUT", "1", "-i", iface, "-p", proto, "--dport", &port_str, "-j", "ACCEPT"])?;
                }
            }
        }

        // 6. Синхронизируем XKEEN_GAMING и XKEEN_BS, если цепочки активны (перехват DNS туннелями на порт AdGuard, баг 1)
        let mihomo_port = crate::routing::mihomo_dns_port_from_path(&crate::config::MihomoConfig::default().config_path);
        let sync_cmd = build_sync_tunnel_dns_ports_script(&format!("{mihomo_port}|5353|5354"), dns_port);
        if let Ok(out) = std::process::Command::new("sh").arg("-c").arg(&sync_cmd).output() {
            if !out.status.success() {
                crate::log_w!("[ADGUARD] Предупреждение синхронизации туннельных DNS-правил: {}", String::from_utf8_lossy(&out.stderr).trim());
            }
        }

        // 7. Устанавливаем NDM hook для защиты от сброса правил фаервола системой KeeneticOS
        let hook_path = std::path::Path::new("/opt/etc/ndm/netfilter.d/040-adguard.sh");
        if let Some(parent) = hook_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let hook_content = build_adguard_ndm_hook_content(dns_port);
        if let Ok(()) = std::fs::write(hook_path, hook_content) {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(hook_path, std::fs::Permissions::from_mode(0o755));
            }
        }
    }
    let _ = dns_port;
    Ok(())
}

/// Определяет порт возврата туннелей (dns.listen Mihomo) при снятии правил AdGuard (баг 1, баг 45).
pub fn resolve_adguard_restore_port(restore_port: Option<u16>, mihomo_config_path: &str) -> u16 {
    if let Some(p) = restore_port {
        return p;
    }
    std::fs::read_to_string(mihomo_config_path)
        .ok()
        .and_then(|y| crate::routing::parse_mihomo_dns_listen_port(&y))
        .unwrap_or(crate::routing::DEFAULT_MIHOMO_DNS_PORT)
}

/// Полное и чистое удаление правил XKEEN_ADGUARD из iptables.
/// Порт возврата туннелей (XKEEN_GAMING, XKEEN_BS) параметризован (баг 1, баг 45).
pub fn remove_adguard_iptables() -> Result<(), String> {
    remove_adguard_iptables_with_restore_port(None)
}

/// Удаление правил XKEEN_ADGUARD с автоматическим определением порта возврата из указанного `config.yaml` Mihomo.
pub fn remove_adguard_iptables_for_mihomo_config(mihomo_config_path: &str) -> Result<(), String> {
    let restore_port = resolve_adguard_restore_port(None, mihomo_config_path);
    remove_adguard_iptables_with_restore_port(Some(restore_port))
}

pub fn remove_adguard_iptables_with_restore_port(restore_port: Option<u16>) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let port = detect_adguard_dns_port().unwrap_or(DEFAULT_ADGUARD_DNS_FALLBACK_PORT);
        let port_str = port.to_string();

        for iface in AGH_LAN_INTERFACES {
            for proto in &["udp", "tcp"] {
                for _ in 0..20 {
                    if !crate::system::run_iptables_ok(&["-t", "nat", "-D", "PREROUTING", "-i", iface, "-p", proto, "--dport", "53", "-j", AGH_IPTABLES_CHAIN]) {
                        break;
                    }
                }
            }
        }
        let mut allowed_input_ifaces = AGH_LAN_INTERFACES.to_vec();
        allowed_input_ifaces.push("lo");
        for iface in allowed_input_ifaces {
            for proto in &["udp", "tcp"] {
                for _ in 0..20 {
                    if !crate::system::run_iptables_ok(&["-t", "filter", "-D", "INPUT", "-i", iface, "-p", proto, "--dport", &port_str, "-j", "ACCEPT"]) {
                        break;
                    }
                }
                for _ in 0..20 {
                    if !crate::system::run_iptables_ok(&["-t", "filter", "-D", "INPUT", "-i", iface, "-p", proto, "--dport", "5353", "-j", "ACCEPT"]) {
                        break;
                    }
                }
            }
        }
        for proto in &["udp", "tcp"] {
            for _ in 0..20 {
                if !crate::system::run_iptables_ok(&["-t", "filter", "-D", "INPUT", "-p", proto, "--dport", &port_str, "-j", "ACCEPT"]) {
                    break;
                }
            }
            // Также подчищаем старый порт 5353, если он был прописан в filter INPUT
            for _ in 0..20 {
                if !crate::system::run_iptables_ok(&["-t", "filter", "-D", "INPUT", "-p", proto, "--dport", "5353", "-j", "ACCEPT"]) {
                    break;
                }
            }
        }
        // Возвращаем XKEEN_GAMING и XKEEN_BS на порт Mihomo при остановке/падении AdGuard (баг 1, баг 45)
        let target_restore_port = resolve_adguard_restore_port(
            restore_port,
            &crate::config::MihomoConfig::default().config_path,
        );
        let restore_cmd = build_sync_tunnel_dns_ports_script(&format!("{port_str}|5353|5354"), target_restore_port);
        let _ = std::process::Command::new("sh")
            .arg("-c")
            .arg(&restore_cmd)
            .output();

        let _ = std::process::Command::new("iptables")
            .args(["-w", "2", "-t", "nat", "-F", AGH_IPTABLES_CHAIN])
            .output();
        let _ = std::process::Command::new("iptables")
            .args(["-w", "2", "-t", "nat", "-X", AGH_IPTABLES_CHAIN])
            .output();

        let hook_path = std::path::Path::new("/opt/etc/ndm/netfilter.d/040-adguard.sh");
        let _ = std::fs::remove_file(hook_path);
    }
    let _ = restore_port;
    Ok(())
}

/// Проверка активности редиректа AdGuard в iptables.
/// G-01: активность = (1) PREROUTING реально прыгает в XKEEN_ADGUARD И (2) цепочка
/// содержит рабочий REDIRECT --to-ports {dns_port}. Раньше совпадение имени цепочки
/// где угодно (существование без jump) или «--dport 53 + to-ports N» в ЛЮБОЙ цепочке
/// (например XKEEN_GAMING с 5353) давало ложный позитив: watchdog считал редирект
/// активным и не восстанавливал его — «тумблер горит, а фильтр не работает».
pub fn is_adguard_iptables_active(dns_port: u16) -> bool {
    #[cfg(target_os = "linux")]
    {
        let prerouting_out = std::process::Command::new("iptables")
            .args(["-w", "2", "-t", "nat", "-S", "PREROUTING"])
            .output();
        let jump_active = prerouting_out
            .ok()
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .any(|l| l.contains(&format!("-j {}", AGH_IPTABLES_CHAIN)))
            })
            .unwrap_or(false);
        if !jump_active {
            return false;
        }

        let chain_out = std::process::Command::new("iptables")
            .args(["-w", "2", "-t", "nat", "-S", AGH_IPTABLES_CHAIN])
            .output();
        let rules = chain_out
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_default();
        let port_str = format!("--to-ports {}", dns_port);
        return rules.contains("--dport 53") && rules.contains(&port_str);
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = dns_port;
        false
    }
}
