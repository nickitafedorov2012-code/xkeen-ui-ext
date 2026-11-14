use axum::extract::{Query, State};
use axum::response::Response;
use serde::Deserialize;
use serde_json::json;

use super::{api_err, api_err_bad_request, api_ok, atomic_write_file};
use crate::{config, log_e, log_w, mihomo, routing, AppState};
// ==================== ZAPRET / DPI ИНТЕГРАЦИЯ =============

pub const DEFAULT_ZAPRET_HOSTS: &str = "# zapret-hosts.txt — Список доменов для универсального обхода DPI\n\
# Поддомены применяются автоматически (subdomains auto apply)\n\
rutracker.org\n\
ntc.party\n\
kinozal.tv\n\
flibusta.is\n\
hdrezka.ag\n\
lostfilm.tv\n\
nnmclub.to\n\
torlook.info\n\
medium.com\n\
linkedin.com\n\
notion.so\n\
canva.com\n\
intel.com\n\
dell.com\n";

pub const NDM_NETFILTER_SCRIPT: &str = r#"#!/bin/sh
PATH=/opt/sbin:/opt/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
export PATH

# Keenetic ndm netfilter hook for Zapret
# Automatically re-injects netfilter rules whenever KeeneticOS rebuilds iptables
# (e.g. DHCP lease renewal, WAN reconnect, ping-check, routing policy reload).

[ "$type" = "ip6" ] || [ "$type" = "ipv6" ] && exit 0
[ "$table" = "filter" ] || [ "$table" = "security" ] && exit 0

get_active_wan_ifaces() {
  if [ -f /proc/net/route ]; then
    awk '$2 == "00000000" && $8 !~ /^lo/ {print $1}' /proc/net/route 2>/dev/null | sort -u
  elif command -v ip >/dev/null 2>&1; then
    ip route show default 2>/dev/null | awk '{for(i=1;i<=NF;i++) if($i=="dev") print $(i+1)}' | sort -u
  fi
}

is_zapret_pid() {
  local p="$1"
  [ -z "$p" ] && return 1
  [ -r "/proc/$p/cmdline" ] || return 1
  local cmdline=$(tr '\0' ' ' < "/proc/$p/cmdline" 2>/dev/null)
  case "$cmdline" in
    *--qnum=200*|*/opt/etc/zapret*|*/opt/zapret*|*xkeen-route*) return 0 ;;
  esac
  return 1
}

is_running() {
  if [ -f /opt/var/run/zapret.pid ]; then
    PID=$(cat /opt/var/run/zapret.pid 2>/dev/null)
    if [ -n "$PID" ] && kill -0 "$PID" 2>/dev/null && is_zapret_pid "$PID"; then return 0; fi
  fi
  for p in $(pidof nfqws2 2>/dev/null) $(pidof nfqws 2>/dev/null); do
    if is_zapret_pid "$p"; then return 0; fi
  done
  return 1
}

is_running || exit 0
[ -x /opt/etc/init.d/S51zapret ] || exit 0

/opt/etc/init.d/S51zapret start-fw >/dev/null 2>&1
exit 0
"#;

pub const NDM_IFSTATE_SCRIPT: &str = r#"#!/bin/sh
PATH=/opt/sbin:/opt/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
export PATH

# Keenetic ndm ifstatechanged hook for Zapret
# Re-applies netfilter rules when interface state changes (WAN connects, DHCP lease renewed).

[ "$state" = "down" ] && exit 0

get_active_wan_ifaces() {
  if [ -f /proc/net/route ]; then
    awk '$2 == "00000000" && $8 !~ /^lo/ {print $1}' /proc/net/route 2>/dev/null | sort -u
  elif command -v ip >/dev/null 2>&1; then
    ip route show default 2>/dev/null | awk '{for(i=1;i<=NF;i++) if($i=="dev") print $(i+1)}' | sort -u
  fi
}

is_zapret_pid() {
  local p="$1"
  [ -z "$p" ] && return 1
  [ -r "/proc/$p/cmdline" ] || return 1
  local cmdline=$(tr '\0' ' ' < "/proc/$p/cmdline" 2>/dev/null)
  case "$cmdline" in
    *--qnum=200*|*/opt/etc/zapret*|*/opt/zapret*|*xkeen-route*) return 0 ;;
  esac
  return 1
}

is_running() {
  if [ -f /opt/var/run/zapret.pid ]; then
    PID=$(cat /opt/var/run/zapret.pid 2>/dev/null)
    if [ -n "$PID" ] && kill -0 "$PID" 2>/dev/null && is_zapret_pid "$PID"; then return 0; fi
  fi
  for p in $(pidof nfqws2 2>/dev/null) $(pidof nfqws 2>/dev/null); do
    if is_zapret_pid "$p"; then return 0; fi
  done
  return 1
}

is_running || exit 0
[ -x /opt/etc/init.d/S51zapret ] || exit 0

/opt/etc/init.d/S51zapret start-fw >/dev/null 2>&1
exit 0
"#;

/// Спецификация очереди NFQUEUE и фильтруемых портов из пользовательских аргументов Zapret (баг 34).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueSpec {
    pub qnum: u16,
    pub tcp_ports: Vec<String>,
    pub udp_ports: Vec<String>,
}

impl Default for QueueSpec {
    fn default() -> Self {
        Self {
            qnum: 200,
            tcp_ports: vec!["80".to_string(), "443".to_string()],
            udp_ports: Vec::new(),
        }
    }
}

fn validate_port_token(token: &str) -> Result<String, String> {
    let t = token.trim();
    if t.is_empty() {
        return Err("Пустой порт в фильтре Zapret".to_string());
    }
    if let Some((a_raw, b_raw)) = t.split_once('-').or_else(|| t.split_once(':')) {
        let a: u16 = a_raw
            .trim()
            .parse()
            .map_err(|_| format!("Некорректный начальный порт в диапазоне '{t}'"))?;
        let b: u16 = b_raw
            .trim()
            .parse()
            .map_err(|_| format!("Некорректный конечный порт в диапазоне '{t}'"))?;
        if a == 0 || b == 0 || a > b {
            return Err(format!("Некорректный диапазон портов '{t}' (допустимо 1-65535)"));
        }
        Ok(format!("{a}-{b}"))
    } else {
        let p: u16 = t
            .parse()
            .map_err(|_| format!("Некорректный порт '{t}' (ожидается число 1-65535)"))?;
        if p == 0 {
            return Err("Порт 0 недопустим (допустимо 1-65535)".to_string());
        }
        Ok(p.to_string())
    }
}

fn parse_port_list(val: &str, flag_name: &str) -> Result<Vec<String>, String> {
    let trimmed = val.trim();
    if trimmed.is_empty() {
        return Err(format!("Пустой список портов для {flag_name}"));
    }
    let mut out = Vec::new();
    for part in trimmed.split(',') {
        let norm = validate_port_token(part)?;
        if !out.contains(&norm) {
            out.push(norm);
        }
    }
    Ok(out)
}

/// Разбирает `--qnum`, `--filter-tcp` и `--filter-udp` из `custom_args` с валидацией 1-65535 (баг 34).
pub fn parse_zapret_queue_spec(custom_args: &str) -> Result<QueueSpec, String> {
    let mut qnum: u16 = 200;
    let mut tcp_ports: Vec<String> = Vec::new();
    let mut udp_ports: Vec<String> = Vec::new();
    let mut saw_tcp = false;
    let mut saw_udp = false;

    let tokens: Vec<&str> = custom_args.split_whitespace().collect();
    let mut i = 0;
    while i < tokens.len() {
        let tok = tokens[i];
        if let Some(val) = tok.strip_prefix("--qnum=") {
            let parsed: u16 = val
                .trim()
                .parse()
                .map_err(|_| format!("Некорректное значение --qnum='{val}' (допустимо 1-65535)"))?;
            if parsed == 0 {
                return Err("Некорректное значение --qnum=0 (допустимо 1-65535)".to_string());
            }
            qnum = parsed;
        } else if tok == "--qnum" {
            let val = tokens
                .get(i + 1)
                .ok_or_else(|| "Отсутствует значение после --qnum".to_string())?;
            let parsed: u16 = val
                .trim()
                .parse()
                .map_err(|_| format!("Некорректное значение --qnum '{val}' (допустимо 1-65535)"))?;
            if parsed == 0 {
                return Err("Некорректное значение --qnum 0 (допустимо 1-65535)".to_string());
            }
            qnum = parsed;
            i += 1;
        } else if let Some(val) = tok.strip_prefix("--filter-tcp=") {
            saw_tcp = true;
            for p in parse_port_list(val, "--filter-tcp")? {
                if !tcp_ports.contains(&p) {
                    tcp_ports.push(p);
                }
            }
        } else if tok == "--filter-tcp" {
            saw_tcp = true;
            let val = tokens
                .get(i + 1)
                .ok_or_else(|| "Отсутствует значение после --filter-tcp".to_string())?;
            for p in parse_port_list(val, "--filter-tcp")? {
                if !tcp_ports.contains(&p) {
                    tcp_ports.push(p);
                }
            }
            i += 1;
        } else if let Some(val) = tok.strip_prefix("--filter-udp=") {
            saw_udp = true;
            for p in parse_port_list(val, "--filter-udp")? {
                if !udp_ports.contains(&p) {
                    udp_ports.push(p);
                }
            }
        } else if tok == "--filter-udp" {
            saw_udp = true;
            let val = tokens
                .get(i + 1)
                .ok_or_else(|| "Отсутствует значение после --filter-udp".to_string())?;
            for p in parse_port_list(val, "--filter-udp")? {
                if !udp_ports.contains(&p) {
                    udp_ports.push(p);
                }
            }
            i += 1;
        }
        i += 1;
    }

    if !saw_tcp && !saw_udp {
        tcp_ports = vec!["80".to_string(), "443".to_string()];
    }

    Ok(QueueSpec {
        qnum,
        tcp_ports,
        udp_ports,
    })
}

fn format_iptables_multiport(ports: &[String]) -> String {
    ports
        .iter()
        .map(|p| p.replace('-', ":"))
        .collect::<Vec<_>>()
        .join(",")
}

/// Сборка S51zapret с учётом DNS-порта Mihomo, исключений игровых IP (баг 33) и QueueSpec (баг 34).
pub fn build_zapret_firewall_script_with_queue(
    dns_port: u16,
    gaming_ips: &[String],
    queue: &QueueSpec,
) -> String {
    let pid_check_fn = crate::zapret::build_zapret_pid_check_fn_with_qnum(queue.qnum);
    let hex_port = format!(":{dns_port:04X}");
    let qnum_str = queue.qnum.to_string();
    let mut gaming_exemptions = String::new();
    for raw_ip in gaming_ips {
        let ip = raw_ip.trim();
        if !ip.is_empty() && crate::config::is_valid_ip_or_cidr(ip) {
            gaming_exemptions.push_str(&format!(
                "    iptables -w -t mangle -A zapret -s {ip} -p udp -m multiport --dports 50000:65535 -m comment --comment \"xkeen-route-zapret\" -j RETURN 2>/dev/null || \\\n\
    iptables -w -t mangle -A zapret -s {ip} -p udp --dport 50000:65535 -j RETURN 2>/dev/null || true\n"
            ));
        }
    }

    let tcp_queue_rules = if queue.tcp_ports.is_empty() {
        String::new()
    } else {
        let dports = format_iptables_multiport(&queue.tcp_ports);
        let mut fallback = String::new();
        for p in &queue.tcp_ports {
            let ipt_p = p.replace('-', ":");
            fallback.push_str(&format!(
                "    iptables -w -t mangle -A zapret -p tcp --dport {ipt_p} -j NFQUEUE --queue-num {qnum_str} --queue-bypass 2>/dev/null || true\n"
            ));
        }
        format!(
            "  if iptables -w -t mangle -A zapret -p tcp -m multiport --dports {dports} -m comment --comment \"xkeen-route-zapret\" -j NFQUEUE --queue-num {qnum_str} --queue-bypass 2>/dev/null || \\\n\
     iptables -w -t mangle -A zapret -p tcp -m multiport --dports {dports} -j NFQUEUE --queue-num {qnum_str} --queue-bypass 2>/dev/null; then\n\
    :\n\
  else\n\
{fallback}  fi\n"
        )
    };

    let udp_queue_rules = if queue.udp_ports.is_empty() {
        String::new()
    } else {
        let dports = format_iptables_multiport(&queue.udp_ports);
        let mut fallback = String::new();
        for p in &queue.udp_ports {
            let ipt_p = p.replace('-', ":");
            fallback.push_str(&format!(
                "    iptables -w -t mangle -A zapret -p udp --dport {ipt_p} -j NFQUEUE --queue-num {qnum_str} --queue-bypass 2>/dev/null || true\n"
            ));
        }
        format!(
            "  if iptables -w -t mangle -A zapret -p udp -m multiport --dports {dports} -m comment --comment \"xkeen-route-zapret\" -j NFQUEUE --queue-num {qnum_str} --queue-bypass 2>/dev/null || \\\n\
     iptables -w -t mangle -A zapret -p udp -m multiport --dports {dports} -j NFQUEUE --queue-num {qnum_str} --queue-bypass 2>/dev/null; then\n\
    :\n\
  else\n\
{fallback}  fi\n"
        )
    };

    let output_tcp_rules = if queue.tcp_ports.is_empty() {
        String::new()
    } else {
        let dports = format_iptables_multiport(&queue.tcp_ports);
        format!(
            "  iptables -w -t mangle -I OUTPUT 1 -m mark ! --mark 0x40000000/0x40000000 -p tcp -m multiport --dports {dports} -m comment --comment \"xkeen-route-zapret\" -j zapret 2>/dev/null || \\\n\
  iptables -w -t mangle -I OUTPUT 1 -m mark ! --mark 0x40000000/0x40000000 -p tcp -m multiport --dports {dports} -j zapret 2>/dev/null || true\n"
        )
    };

    S51ZAPRET_SCRIPT
        .replace("__ZAPRET_PID_CHECK_FN__", &pid_check_fn)
        .replace("__ZAPRET_DNS_PORT_HEX__", &hex_port)
        .replace("__ZAPRET_DNS_PORT__", &dns_port.to_string())
        .replace("__ZAPRET_QNUM__", &qnum_str)
        .replace("__ZAPRET_TCP_QUEUE_RULES__", &tcp_queue_rules)
        .replace("__ZAPRET_UDP_QUEUE_RULES__", &udp_queue_rules)
        .replace("__ZAPRET_OUTPUT_TCP_RULES__", &output_tcp_rules)
        .replace("__ZAPRET_GAMING_UDP_EXEMPTIONS__", &gaming_exemptions)
}

/// Сборка S51zapret с учётом DNS-порта Mihomo и исключений игровых IP от Discord Voice NFQUEUE (баг 33).
pub fn build_zapret_firewall_script(dns_port: u16, gaming_ips: &[String]) -> String {
    build_zapret_firewall_script_with_queue(dns_port, gaming_ips, &QueueSpec::default())
}

pub fn render_s51_zapret_script_with_dns_port(dns_port: u16) -> String {
    build_zapret_firewall_script(dns_port, &[])
}

pub const S51ZAPRET_SCRIPT: &str = r#"#!/bin/sh
PATH=/opt/sbin:/opt/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
export PATH

IPTABLES_CMD="iptables"
if command iptables -w 1 -L -n >/dev/null 2>&1; then
  IPTABLES_CMD="iptables -w 2"
fi

iptables() {
  command $IPTABLES_CMD "$@"
}

PIDFILE="/opt/var/run/zapret.pid"
FAILSAFE_PID="/opt/var/run/zapret_failsafe.pid"
CONF="/opt/etc/zapret/zapret.conf"
LOCKDIR="/opt/var/run/xkeen-zapret.lock.d"
DISABLED_MARKER="/opt/var/run/xkeen-zapret.disabled"

acquire_zapret_lock() {
  # Единый с Rust-панелью протокол блокировки: mkdir lockdir + pid-файл внутри (Z-01).
  # flock на файл не используется — он не пересекался с mkdir-локом панели.
  local timeout=15
  local waited=0
  while ! mkdir "$LOCKDIR" 2>/dev/null; do
    local holder=$(cat "$LOCKDIR/pid" 2>/dev/null)
    if [ -n "$holder" ] && ! kill -0 "$holder" 2>/dev/null; then
      rm -rf "$LOCKDIR" 2>/dev/null
      continue
    fi
    if [ "$waited" -ge "$timeout" ]; then
      echo "Failed to acquire zapret lockdir after ${timeout}s" >&2
      return 1
    fi
    sleep 1
    waited=$((waited + 1))
  done
  echo "$$" > "$LOCKDIR/pid" 2>/dev/null
  return 0
}

release_zapret_lock() {
  rm -rf "$LOCKDIR" 2>/dev/null
}

__ZAPRET_PID_CHECK_FN__

check_dns_resolver_ready() {
  # Z-06: порт слушателя ищем строго в колонке local_address ($2) — раньше grep ':041D'
  # по всему файлу матчил и ЧУЖИЕ remote-порты, что включало DNS-редирект
  # без работающего резолвера (блэкаут LAN).
  if awk '$2 ~ /__ZAPRET_DNS_PORT_HEX__$/ { found=1 } END { exit !found }' /proc/net/udp /proc/net/udp6 2>/dev/null; then
    return 0
  fi
  if awk '$2 ~ /__ZAPRET_DNS_PORT_HEX__$/ { found=1 } END { exit !found }' /proc/net/tcp /proc/net/tcp6 2>/dev/null; then
    return 0
  fi
  if command -v netstat >/dev/null 2>&1 && netstat -tuln 2>/dev/null | grep -q ':__ZAPRET_DNS_PORT__ '; then
    return 0
  fi
  return 1
}

get_active_wan_ifaces() {
  if [ -f /proc/net/route ]; then
    awk '$2 == "00000000" && $8 !~ /^lo/ {print $1}' /proc/net/route 2>/dev/null | sort -u
  elif command -v ip >/dev/null 2>&1; then
    ip route show default 2>/dev/null | awk '{for(i=1;i<=NF;i++) if($i=="dev") print $(i+1)}' | sort -u
  fi
}

find_bin() {
  ARCH=$(uname -m 2>/dev/null)
  case "$ARCH" in
    aarch64|arm64)
      ARCH_CANDIDATES="arm64 linux-arm64"
      ;;
    armv7*|armv8*|arm*)
      ARCH_CANDIDATES="arm linux-arm"
      ;;
    mips*)
      if [ "$(hexdump -s 5 -n 1 -e '"%02x"' /bin/sh 2>/dev/null)" = "02" ] || \
         [ "$(od -t x1 -j 5 -N 1 /bin/sh 2>/dev/null | awk 'NR==1{print $2}')" = "02" ] || \
         [ "$(echo -n I | hexdump -o 2>/dev/null | awk '{ print substr($2,6,1); exit }')" = "0" ]; then
        ARCH_CANDIDATES="mips linux-mips32r2-msb mipsel linux-mips32r2-lsb"
      else
        ARCH_CANDIDATES="mipsel linux-mips32r2-lsb mips linux-mips32r2-msb"
      fi
      ;;
    x86_64|amd64)
      ARCH_CANDIDATES="x86_64 linux-x86_64"
      ;;
    i*86|x86)
      ARCH_CANDIDATES="x86 linux-x86"
      ;;
    *)
      ARCH_CANDIDATES="mipsel arm64 arm mips x86_64"
      ;;
  esac

  is_runnable() {
    [ -n "$1" ] && [ -x "$1" ] && "$1" --help >/dev/null 2>&1
    local ret=$?
    [ $ret -ne 126 ] && [ $ret -ne 127 ]
  }

  if [ "$ZAPRET_ENGINE" = "v1" ] || [ "$ZAPRET_ENGINE" = "legacy" ]; then
    if [ -x "/opt/zapret/nfq/nfqws" ] && is_runnable "/opt/zapret/nfq/nfqws"; then
      echo "/opt/zapret/nfq/nfqws"
      return
    fi
    for cand in $ARCH_CANDIDATES; do
      if [ -x "/opt/zapret/binaries/$cand/nfqws" ] && is_runnable "/opt/zapret/binaries/$cand/nfqws"; then
        echo "/opt/zapret/binaries/$cand/nfqws"
        return
      fi
    done
    for std_bin in /opt/sbin/nfqws /opt/bin/nfqws /opt/usr/bin/nfqws /opt/zapret/nfqws.bak; do
      if [ -x "$std_bin" ] && is_runnable "$std_bin"; then
        echo "$std_bin"
        return
      fi
    done
    if [ -x "/opt/zapret2/nfqws2" ] && is_runnable "/opt/zapret2/nfqws2"; then
      echo "/opt/zapret2/nfqws2"
      return
    elif [ -x "/opt/sbin/nfqws2" ] && is_runnable "/opt/sbin/nfqws2"; then
      echo "/opt/sbin/nfqws2"
      return
    fi
    return
  fi

  if [ -x "/opt/zapret2/nfqws2" ] && is_runnable "/opt/zapret2/nfqws2"; then
    echo "/opt/zapret2/nfqws2"
    return
  fi
  for std_z2 in /opt/sbin/nfqws2 /opt/bin/nfqws2 /opt/usr/bin/nfqws2; do
    if [ -x "$std_z2" ] && is_runnable "$std_z2"; then
      echo "$std_z2"
      return
    fi
  done
  for cand in $ARCH_CANDIDATES; do
    if [ -x "/opt/zapret2/binaries/$cand/nfqws2" ] && is_runnable "/opt/zapret2/binaries/$cand/nfqws2"; then
      cp -f "/opt/zapret2/binaries/$cand/nfqws2" /opt/zapret2/nfqws2 2>/dev/null || true
      chmod +x /opt/zapret2/nfqws2 2>/dev/null || true
      echo "/opt/zapret2/binaries/$cand/nfqws2"
      return
    fi
  done
  for cand in $ARCH_CANDIDATES; do
    if [ -x "/opt/zapret/binaries/$cand/nfqws" ] && is_runnable "/opt/zapret/binaries/$cand/nfqws"; then
      echo "/opt/zapret/binaries/$cand/nfqws"
      return
    fi
  done
  if [ -x "/opt/zapret/nfq/nfqws" ] && is_runnable "/opt/zapret/nfq/nfqws"; then
    echo "/opt/zapret/nfq/nfqws"
    return
  fi
}

# Fallback default with fwmark to prevent loops
ZAPRET_ENGINE="v2"
NFQWS_ARGS="--daemon --qnum=200 --dpi-desync-fwmark=0x40000000 --filter-tcp=80,443 --hostlist-domains=googlevideo.com,youtube.com,ytimg.com,ggpht.com,youtu.be,yt.be,youtube-nocookie.com,discord.com,discord.gg,discordapp.com --dpi-desync=fake,split2 --dpi-desync-split-pos=1 --dpi-desync-repeats=6 --dpi-desync-fooling=ts --dpi-desync-cutoff=d4"
DISCORD_VOICE_ENABLED="1"
BLOCK_QUIC="0"
SMART_TV_MODE="0"
EXCLUDED_IPS=""
EXCLUDED_MACS=""
ADGUARD_ENABLED="0"

# Safe key-value parser for zapret.conf (no source / eval)
if [ -f "$CONF" ]; then
  while IFS='=' read -r key val || [ -n "$key" ]; do
    key=$(echo "$key" | tr -d ' \t\r\n')
    val=$(echo "$val" | sed -e 's/^[ \t]*//' -e 's/[ \t\r\n]*$//')
    case "$key" in
      ZAPRET_ENGINE)
        val="${val#\"}"
        val="${val%\"}"
        val="${val#\'}"
        val="${val%\'}"
        ZAPRET_ENGINE="$val"
        ;;
      NFQWS_ARGS)
        val="${val#\"}"
        val="${val%\"}"
        val="${val#\'}"
        val="${val%\'}"
        NFQWS_ARGS="$val"
        ;;
      DISCORD_VOICE_ENABLED)
        val="${val#\"}"
        val="${val%\"}"
        val="${val#\'}"
        val="${val%\'}"
        DISCORD_VOICE_ENABLED="$val"
        ;;
      BLOCK_QUIC)
        val="${val#\"}"
        val="${val%\"}"
        val="${val#\'}"
        val="${val%\'}"
        BLOCK_QUIC="$val"
        ;;
      SMART_TV_MODE)
        val="${val#\"}"
        val="${val%\"}"
        val="${val#\'}"
        val="${val%\'}"
        SMART_TV_MODE="$val"
        ;;
      EXCLUDED_IPS)
        val="${val#\"}"
        val="${val%\"}"
        val="${val#\'}"
        val="${val%\'}"
        EXCLUDED_IPS="$val"
        ;;
      EXCLUDED_MACS)
        val="${val#\"}"
        val="${val%\"}"
        val="${val#\'}"
        val="${val%\'}"
        EXCLUDED_MACS="$val"
        ;;
      ADGUARD_ENABLED)
        val="${val#\"}"
        val="${val%\"}"
        val="${val#\'}"
        val="${val%\'}"
        ADGUARD_ENABLED="$val"
        ;;
    esac
  done < "$CONF"
fi

check_adguard_active() {
  [ "$ADGUARD_ENABLED" = "1" ] && return 0
  if pgrep -f AdGuardHome >/dev/null 2>&1; then
    return 0
  fi
  return 1
}

BIN=$(find_bin)

stop_nfqws() {
  if [ -f "$PIDFILE" ]; then
    PID=$(cat "$PIDFILE" 2>/dev/null)
    if [ -n "$PID" ] && is_zapret_pid "$PID"; then
      kill -15 "$PID" 2>/dev/null
      for i in 1 2 3 4 5; do
        if ! is_zapret_pid "$PID"; then break; fi
        sleep 1
      done
      if is_zapret_pid "$PID"; then
        kill -9 "$PID" 2>/dev/null
      fi
    fi
    rm -f "$PIDFILE"
  fi
  for p in $(pgrep -f nfqws 2>/dev/null) $(pgrep -f nfqws2 2>/dev/null) $(pgrep -f tpws 2>/dev/null); do
    if is_zapret_pid "$p"; then
      kill -15 "$p" 2>/dev/null
    fi
  done
}

add_fw() {
  remove_fw_rules

  # Dedicated zapret chain in mangle
  iptables -t mangle -N zapret 2>/dev/null
  iptables -t mangle -F zapret || { echo "ERROR: failed to flush mangle zapret chain" >&2; remove_fw_rules; return 1; }

  # 1. CRITICAL: Skip packets already marked by nfqws to prevent infinite packet looping
  iptables -t mangle -A zapret -m mark --mark 0x40000000/0x40000000 -j RETURN || { echo "ERROR: failed to add fwmark bypass rule in mangle zapret" >&2; remove_fw_rules; return 1; }

  # 2. Exclude loopback and LAN bridge (OpenWrt br+ and Keenetic Bridge+)
  iptables -t mangle -A zapret -i lo -m comment --comment "xkeen-route-zapret" -j RETURN 2>/dev/null || \
  iptables -t mangle -A zapret -i lo -j RETURN 2>/dev/null || true
  iptables -t mangle -A zapret -o lo -m comment --comment "xkeen-route-zapret" -j RETURN 2>/dev/null || \
  iptables -t mangle -A zapret -o lo -j RETURN 2>/dev/null || true
  iptables -t mangle -A zapret -o br+ -m comment --comment "xkeen-route-zapret" -j RETURN 2>/dev/null || \
  iptables -t mangle -A zapret -o br+ -j RETURN 2>/dev/null || true
  iptables -t mangle -A zapret -o Bridge+ -m comment --comment "xkeen-route-zapret" -j RETURN 2>/dev/null || \
  iptables -t mangle -A zapret -o Bridge+ -j RETURN 2>/dev/null || true

  # 3. CRITICAL: Skip private/local subnets & router IP so Keenetic Web UI / LAN are NEVER touched
  for net in 0.0.0.0/8 10.0.0.0/8 100.64.0.0/10 127.0.0.0/8 169.254.0.0/16 172.16.0.0/12 192.168.0.0/16 224.0.0.0/4 240.0.0.0/4 255.255.255.255/32; do
    iptables -t mangle -A zapret -d "$net" -m comment --comment "xkeen-route-zapret" -j RETURN 2>/dev/null || \
    iptables -t mangle -A zapret -d "$net" -j RETURN 2>/dev/null || true
  done

  # 3.1 Per-Device Zapret: exclude devices where Zapret is disabled
  if [ -n "$EXCLUDED_IPS" ]; then
    for ex_ip in $EXCLUDED_IPS; do
      [ -n "$ex_ip" ] || continue
      iptables -t mangle -A zapret -s "$ex_ip" -m comment --comment "xkeen-route-zapret" -j RETURN 2>/dev/null || \
      iptables -t mangle -A zapret -s "$ex_ip" -j RETURN 2>/dev/null || true
      iptables -t mangle -A zapret -d "$ex_ip" -m comment --comment "xkeen-route-zapret" -j RETURN 2>/dev/null || \
      iptables -t mangle -A zapret -d "$ex_ip" -j RETURN 2>/dev/null || true
    done
  fi

  # 3.2 Per-Device Zapret (MAC): exclude devices by hardware MAC address
  if [ -n "$EXCLUDED_MACS" ]; then
    for ex_mac in $EXCLUDED_MACS; do
      [ -n "$ex_mac" ] || continue
      iptables -t mangle -A zapret -m mac --mac-source "$ex_mac" -m comment --comment "xkeen-route-zapret" -j RETURN 2>/dev/null || true
      mac_lower=$(echo "$ex_mac" | tr '[:upper:]' '[:lower:]')
      resolved_ip=$(awk -v m="$mac_lower" 'tolower($4) == m {print $1; exit}' /proc/net/arp 2>/dev/null)
      if [ -n "$resolved_ip" ]; then
        iptables -t mangle -A zapret -s "$resolved_ip" -m comment --comment "xkeen-route-zapret" -j RETURN 2>/dev/null || \
        iptables -t mangle -A zapret -s "$resolved_ip" -j RETURN 2>/dev/null || true
        iptables -t mangle -A zapret -d "$resolved_ip" -m comment --comment "xkeen-route-zapret" -j RETURN 2>/dev/null || \
        iptables -t mangle -A zapret -d "$resolved_ip" -j RETURN 2>/dev/null || true
      fi
    done
  fi

  # 3.3 NET-02: TCP MSS Clamping in POSTROUTING & FORWARD (xt_TCPMSS is forbidden in PREROUTING by kernel)
  iptables -t mangle -A POSTROUTING -p tcp --tcp-flags SYN,RST SYN -m comment --comment "xkeen-route-zapret" -j TCPMSS --clamp-mss-to-pmtu 2>/dev/null || \
  iptables -t mangle -A POSTROUTING -p tcp --tcp-flags SYN,RST SYN -j TCPMSS --clamp-mss-to-pmtu 2>/dev/null || true
  iptables -t mangle -A FORWARD -p tcp --tcp-flags SYN,RST SYN -m comment --comment "xkeen-route-zapret" -j TCPMSS --clamp-mss-to-pmtu 2>/dev/null || \
  iptables -t mangle -A FORWARD -p tcp --tcp-flags SYN,RST SYN -j TCPMSS --clamp-mss-to-pmtu 2>/dev/null || true

  # 4. Queue WAN TCP/UDP -> NFQUEUE __ZAPRET_QNUM__ with bypass
__ZAPRET_TCP_QUEUE_RULES____ZAPRET_UDP_QUEUE_RULES__
  # 5. Drop UDP 443 (QUIC / HTTP3) if enabled or Smart TV mode active (prevents TV player stalls)
  if [ "$BLOCK_QUIC" = "1" ] || [ "$SMART_TV_MODE" = "1" ]; then
    iptables -t mangle -A zapret -p udp --dport 443 -m comment --comment "xkeen-route-zapret" -j DROP 2>/dev/null || \
    iptables -t mangle -A zapret -p udp --dport 443 -j DROP 2>/dev/null || true
  fi

  # 6. Discord Voice RTC UDP (50000:65535) if voice enabled
  if [ "$DISCORD_VOICE_ENABLED" = "1" ]; then
__ZAPRET_GAMING_UDP_EXEMPTIONS__    iptables -t mangle -A zapret -p udp -m multiport --dports 50000:65535 -m comment --comment "xkeen-route-zapret" -j NFQUEUE --queue-num __ZAPRET_QNUM__ --queue-bypass 2>/dev/null || \
    iptables -t mangle -A zapret -p udp --dport 50000:65535 -j NFQUEUE --queue-num __ZAPRET_QNUM__ --queue-bypass 2>/dev/null || true
  fi

  # Hook into PREROUTING for client LAN bridge packets (both br+ and Bridge+) (HC-03)
  for iface in br+ Bridge+; do
    iptables -t mangle -I PREROUTING 1 -i "$iface" -m comment --comment "xkeen-route-zapret" -j zapret 2>/dev/null || \
    iptables -t mangle -I PREROUTING 1 -i "$iface" -j zapret 2>/dev/null || true
  done

  # Hook router-local outbound traffic (e.g., Mihomo DIRECT outbound connections) to Zapret
__ZAPRET_OUTPUT_TCP_RULES__

  # Z-02 & NET-01: Guard DNS redirect before PREROUTING to port __ZAPRET_DNS_PORT__ to prevent LAN blackout & loops
  # CRITICAL: If AdGuard Home is active, DO NOT redirect DNS port 53 to __ZAPRET_DNS_PORT__.
  # AdGuard Home must receive client queries on port 53 for local DNS filtering and ad blocking.
  if ! check_adguard_active && check_dns_resolver_ready; then
    iptables -t nat -A PREROUTING -i br+ -d 127.0.0.1 -m comment --comment "xkeen-route-zapret" -j RETURN 2>/dev/null || \
    iptables -t nat -A PREROUTING -i br+ -d 127.0.0.1 -j RETURN 2>/dev/null || true
    iptables -t nat -A PREROUTING -i Bridge+ -d 127.0.0.1 -m comment --comment "xkeen-route-zapret" -j RETURN 2>/dev/null || \
    iptables -t nat -A PREROUTING -i Bridge+ -d 127.0.0.1 -j RETURN 2>/dev/null || true
    if [ -n "$EXCLUDED_IPS" ]; then
      for ex_ip in $EXCLUDED_IPS; do
        [ -n "$ex_ip" ] || continue
        iptables -t nat -A PREROUTING -i br+ -s "$ex_ip" -p udp --dport 53 -m comment --comment "xkeen-route-zapret" -j RETURN 2>/dev/null || \
        iptables -t nat -A PREROUTING -i br+ -s "$ex_ip" -p udp --dport 53 -j RETURN 2>/dev/null || true
        iptables -t nat -A PREROUTING -i br+ -s "$ex_ip" -p tcp --dport 53 -m comment --comment "xkeen-route-zapret" -j RETURN 2>/dev/null || \
        iptables -t nat -A PREROUTING -i br+ -s "$ex_ip" -p tcp --dport 53 -j RETURN 2>/dev/null || true
        iptables -t nat -A PREROUTING -i Bridge+ -s "$ex_ip" -p udp --dport 53 -m comment --comment "xkeen-route-zapret" -j RETURN 2>/dev/null || \
        iptables -t nat -A PREROUTING -i Bridge+ -s "$ex_ip" -p udp --dport 53 -j RETURN 2>/dev/null || true
        iptables -t nat -A PREROUTING -i Bridge+ -s "$ex_ip" -p tcp --dport 53 -m comment --comment "xkeen-route-zapret" -j RETURN 2>/dev/null || \
        iptables -t nat -A PREROUTING -i Bridge+ -s "$ex_ip" -p tcp --dport 53 -j RETURN 2>/dev/null || true
      done
    fi
    iptables -t nat -A PREROUTING -i br+ -p udp --dport 53 -m comment --comment "xkeen-route-zapret" -j REDIRECT --to-ports __ZAPRET_DNS_PORT__ 2>/dev/null || \
    iptables -t nat -A PREROUTING -i br+ -p udp --dport 53 -j REDIRECT --to-ports __ZAPRET_DNS_PORT__ 2>/dev/null || true
    iptables -t nat -A PREROUTING -i br+ -p tcp --dport 53 -m comment --comment "xkeen-route-zapret" -j REDIRECT --to-ports __ZAPRET_DNS_PORT__ 2>/dev/null || \
    iptables -t nat -A PREROUTING -i br+ -p tcp --dport 53 -j REDIRECT --to-ports __ZAPRET_DNS_PORT__ 2>/dev/null || true
    iptables -t nat -A PREROUTING -i Bridge+ -p udp --dport 53 -m comment --comment "xkeen-route-zapret" -j REDIRECT --to-ports __ZAPRET_DNS_PORT__ 2>/dev/null || \
    iptables -t nat -A PREROUTING -i Bridge+ -p udp --dport 53 -j REDIRECT --to-ports __ZAPRET_DNS_PORT__ 2>/dev/null || true
    iptables -t nat -A PREROUTING -i Bridge+ -p tcp --dport 53 -m comment --comment "xkeen-route-zapret" -j REDIRECT --to-ports __ZAPRET_DNS_PORT__ 2>/dev/null || \
    iptables -t nat -A PREROUTING -i Bridge+ -p tcp --dport 53 -j REDIRECT --to-ports __ZAPRET_DNS_PORT__ 2>/dev/null || true
  else
    if check_adguard_active; then
      echo "AdGuard Home is active; DNS redirection is managed exclusively by XKEEN_ADGUARD chain" >&2
    else
      echo "DNS resolver port __ZAPRET_DNS_PORT__ not listening; client DNS redirect skipped to prevent LAN blackout" >&2
    fi
  fi

  return 0
}

check_connectivity() {
  if command -v curl >/dev/null 2>&1; then
    curl -s -m 5 -o /dev/null http://cp.cloudflare.com/generate_204 2>/dev/null || \
    curl -s -m 5 -o /dev/null http://connectivitycheck.gstatic.com/generate_204 2>/dev/null || \
    curl -s -m 5 -o /dev/null http://www.gstatic.com/generate_204 2>/dev/null || \
    curl -s -m 5 -o /dev/null http://cp.cloudflare.com 2>/dev/null || \
    curl -s -m 5 -o /dev/null http://detectportal.firefox.com/success.txt 2>/dev/null || \
    curl -s -m 5 -o /dev/null http://ya.ru 2>/dev/null
  elif command -v wget >/dev/null 2>&1; then
    wget -q -O /dev/null -T 5 http://cp.cloudflare.com/generate_204 2>/dev/null || \
    wget -q -O /dev/null -T 5 http://connectivitycheck.gstatic.com/generate_204 2>/dev/null || \
    wget -q -O /dev/null -T 5 http://ya.ru 2>/dev/null
  else
    return 0
  fi
}

start_failsafe() {
  if [ -f "$FAILSAFE_PID" ]; then
    kill -9 $(cat "$FAILSAFE_PID") 2>/dev/null
    rm -f "$FAILSAFE_PID"
  fi

  # NET-03 & Z-05: Failsafe timer (45s base with hardware-aware checks)
  (
    sleep 45
    if ! check_connectivity && ! ping -c 1 -W 2 77.88.8.8 >/dev/null 2>&1 && ! ping -c 1 -W 2 8.8.8.8 >/dev/null 2>&1; then
      logger -t zapret "FAILSAFE: internet connectivity lost after enabling zapret, iptables rules rolled back"
      remove_fw_rules
      stop_nfqws
    fi
    rm -f "$FAILSAFE_PID"
  ) </dev/null >/dev/null 2>&1 &
  echo $! > "$FAILSAFE_PID"
}

remove_fw_rules() {
  if [ -f "$FAILSAFE_PID" ]; then
    kill -9 $(cat "$FAILSAFE_PID") 2>/dev/null
    rm -f "$FAILSAFE_PID"
  fi

  # Bug 27: RETURN-правила DNS-защиты (свои — по комментарию, старые — без него)
  iptables -t nat -S PREROUTING 2>/dev/null | grep 'xkeen-route-zapret' | grep -- '-j RETURN' | sed 's/^-A /-D /' | while read -r rule; do
    eval "iptables -t nat $rule" 2>/dev/null || true
  done
  while iptables -t nat -D PREROUTING -i br+ -d 127.0.0.1 -j RETURN 2>/dev/null; do :; done
  while iptables -t nat -D PREROUTING -i Bridge+ -d 127.0.0.1 -j RETURN 2>/dev/null; do :; done
  for ex_ip in $EXCLUDED_IPS; do
    [ -n "$ex_ip" ] || continue
    for iface in br+ Bridge+; do
      while iptables -t nat -D PREROUTING -i "$iface" -s "$ex_ip" -p udp --dport 53 -j RETURN 2>/dev/null; do :; done
      while iptables -t nat -D PREROUTING -i "$iface" -s "$ex_ip" -p tcp --dport 53 -j RETURN 2>/dev/null; do :; done
    done
  done

  # Remove DNS redirects with our comment (Bug 31: only own rules)
  while iptables -w 2 -t nat -D PREROUTING -i br+ -p udp --dport 53 -m comment --comment "xkeen-route-zapret" -j REDIRECT --to-ports __ZAPRET_DNS_PORT__ 2>/dev/null; do :; done
  while iptables -w 2 -t nat -D PREROUTING -i br+ -p tcp --dport 53 -m comment --comment "xkeen-route-zapret" -j REDIRECT --to-ports __ZAPRET_DNS_PORT__ 2>/dev/null; do :; done
  while iptables -w 2 -t nat -D PREROUTING -i Bridge+ -p udp --dport 53 -m comment --comment "xkeen-route-zapret" -j REDIRECT --to-ports __ZAPRET_DNS_PORT__ 2>/dev/null; do :; done
  while iptables -w 2 -t nat -D PREROUTING -i Bridge+ -p tcp --dport 53 -m comment --comment "xkeen-route-zapret" -j REDIRECT --to-ports __ZAPRET_DNS_PORT__ 2>/dev/null; do :; done


  # Remove TCPMSS clamping rule (NET-02 / NET-04)
  while iptables -t mangle -D zapret -p tcp --tcp-flags SYN,RST SYN -j TCPMSS --clamp-mss-to-pmtu 2>/dev/null; do :; done
  while iptables -t mangle -D POSTROUTING -p tcp --tcp-flags SYN,RST SYN -m comment --comment "xkeen-route-zapret" -j TCPMSS --clamp-mss-to-pmtu 2>/dev/null; do :; done
  while iptables -t mangle -D POSTROUTING -p tcp --tcp-flags SYN,RST SYN -j TCPMSS --clamp-mss-to-pmtu 2>/dev/null; do :; done
  while iptables -t mangle -D FORWARD -p tcp --tcp-flags SYN,RST SYN -m comment --comment "xkeen-route-zapret" -j TCPMSS --clamp-mss-to-pmtu 2>/dev/null; do :; done
  while iptables -t mangle -D FORWARD -p tcp --tcp-flags SYN,RST SYN -j TCPMSS --clamp-mss-to-pmtu 2>/dev/null; do :; done

  # Remove hooks from all possible chains (POSTROUTING, PREROUTING, FORWARD, OUTPUT)
  while iptables -t mangle -D POSTROUTING -m comment --comment "xkeen-route-zapret" -j zapret 2>/dev/null; do :; done
  while iptables -t mangle -D POSTROUTING -j zapret 2>/dev/null; do :; done
  while iptables -t mangle -D PREROUTING -i br+ -m comment --comment "xkeen-route-zapret" -j zapret 2>/dev/null; do :; done
  while iptables -t mangle -D PREROUTING -i br+ -j zapret 2>/dev/null; do :; done
  while iptables -t mangle -D PREROUTING -i Bridge+ -m comment --comment "xkeen-route-zapret" -j zapret 2>/dev/null; do :; done
  while iptables -t mangle -D PREROUTING -i Bridge+ -j zapret 2>/dev/null; do :; done
  while iptables -t mangle -D PREROUTING -j zapret 2>/dev/null; do :; done
  while iptables -t mangle -D FORWARD -m comment --comment "xkeen-route-zapret" -j zapret 2>/dev/null; do :; done
  while iptables -t mangle -D FORWARD -j zapret 2>/dev/null; do :; done
  while iptables -t mangle -D OUTPUT -m comment --comment "xkeen-route-zapret" -j zapret 2>/dev/null; do :; done
  while iptables -t mangle -D OUTPUT -j zapret 2>/dev/null; do :; done

  # Flush and delete chain
  iptables -t mangle -F zapret 2>/dev/null
  iptables -t mangle -X zapret 2>/dev/null
  return 0
}

del_fw() {
  remove_fw_rules
}

case "$1" in
  start)
    acquire_zapret_lock || exit 1
    # Z-03: маркер «выключено пользователем» запрещает автозапуск Entware после ребута
    if [ -f "$DISABLED_MARKER" ]; then
      echo "Zapret disabled by user (marker present); ensuring service is stopped"
      stop_nfqws
      remove_fw_rules
      release_zapret_lock
      exit 0
    fi
    mkdir -p /opt/var/run /opt/etc/zapret
    if [ -f "$PIDFILE" ]; then
      PID=$(cat "$PIDFILE" 2>/dev/null)
      if [ -n "$PID" ] && is_zapret_pid "$PID"; then
        add_fw
        start_failsafe
        release_zapret_lock
        exit 0
      fi
    fi
    stop_nfqws
    if [ -n "$BIN" ] && [ -x "$BIN" ]; then
      IS_Z2=0
      case "$BIN" in
        *nfqws2*)
          if "$BIN" --help 2>&1 | grep -q "lua-desync"; then
            IS_Z2=1
          fi
          ;;
      esac
      if [ "$IS_Z2" = "1" ]; then
        mkdir -p /opt/zapret2/lua
        for alt_lua in /opt/zapret/lua /opt/zapret/files/lua /opt/share/zapret/lua; do
          if [ ! -f /opt/zapret2/lua/zapret-lib.lua ] && [ -f "$alt_lua/zapret-lib.lua" ]; then
            cp -rf "$alt_lua/"* /opt/zapret2/lua/ 2>/dev/null || true
            break
          fi
        done
        if [ ! -f "/opt/zapret2/lua/zapret-lib.lua" ] || [ ! -f "/opt/zapret2/lua/zapret-antidpi.lua" ]; then
          logger -t zapret "ERROR: zapret2 Lua libraries missing in /opt/zapret2/lua/"
          echo "ERROR: zapret2 Lua libraries missing in /opt/zapret2/lua/" >&2
          release_zapret_lock
          exit 1
        fi
        for chk in /opt/zapret2/lua/zapret-lib.lua /opt/zapret2/lua/zapret-antidpi.lua; do
          if [ $(wc -c < "$chk" 2>/dev/null || echo 0) -lt 80 ] || grep -q "404: Not Found" "$chk" 2>/dev/null; then
            logger -t zapret "ERROR: Lua library $chk is corrupted or incomplete in /opt/zapret2/lua/"
            echo "ERROR: Lua library $chk is corrupted or incomplete in /opt/zapret2/lua/" >&2
            release_zapret_lock
            exit 1
          fi
        done
      fi
      set -f
      if [ "$IS_Z2" = "1" ]; then
        LUA_INIT_ARG=""
        for lmod in zapret-lib.lua zapret-antidpi.lua zapret-auto.lua; do
          if [ -f "/opt/zapret2/lua/$lmod" ]; then
            LUA_INIT_ARG="$LUA_INIT_ARG --lua-init=@/opt/zapret2/lua/$lmod"
          fi
        done
        case "$NFQWS_ARGS" in
          *--daemon*) ;;
          *) NFQWS_ARGS="--daemon --qnum=200 --dpi-desync-fwmark=0x40000000 $NFQWS_ARGS" ;;
        esac
        if "$BIN" --help 2>&1 | grep -q -- "--dpi-desync-fwmark"; then
          EFFECTIVE_ARGS=$(echo "$NFQWS_ARGS" | sed 's/--fwmark=/--dpi-desync-fwmark=/g')
        else
          EFFECTIVE_ARGS=$(echo "$NFQWS_ARGS" | sed 's/--dpi-desync-fwmark=/--fwmark=/g')
        fi
        $BIN --pidfile="$PIDFILE" $LUA_INIT_ARG $EFFECTIVE_ARGS
      else
        case "$NFQWS_ARGS" in
          *lua-desync*|*payload=*|*out-range=*|*multisplit*|*--fwmark=*)
            logger -t zapret "WARNING: NFQWS_ARGS contains nfqws2 Lua parameters, but running legacy nfqws. Using safe fallback args."
            NFQWS_ARGS="--daemon --qnum=200 --dpi-desync-fwmark=0x40000000 --filter-tcp=80,443 --hostlist-domains=googlevideo.com,youtube.com,ytimg.com,ggpht.com,youtu.be,yt.be,youtube-nocookie.com,discord.com,discord.gg,discordapp.com --dpi-desync=fake,split2 --dpi-desync-split-pos=1 --dpi-desync-repeats=6 --dpi-desync-fooling=ts --dpi-desync-cutoff=d4"
            [ -f "/opt/etc/zapret/zapret-hosts.txt" ] && NFQWS_ARGS="$NFQWS_ARGS --new --filter-tcp=80,443 --hostlist=/opt/etc/zapret/zapret-hosts.txt --dpi-desync=fake,split2 --dpi-desync-split-pos=1 --dpi-desync-repeats=6 --dpi-desync-fooling=ts --dpi-desync-cutoff=d4"
            ;;
        esac
        case "$NFQWS_ARGS" in
          *--daemon*) ;;
          *) NFQWS_ARGS="--daemon --qnum=200 --dpi-desync-fwmark=0x40000000 $NFQWS_ARGS" ;;
        esac
        EFFECTIVE_ARGS=$(echo "$NFQWS_ARGS" | sed 's/--fwmark=/--dpi-desync-fwmark=/g')
        $BIN --pidfile="$PIDFILE" $EFFECTIVE_ARGS
      fi
      set +f
      # Z-20: ждём до 5 сек записи pidfile — на MIPS nfqws стартует дольше 1 сек,
      # фиксированный sleep давал ложный «failed to start» и валил живую службу
      PID=""
      for _wait_i in 1 2 3 4 5; do
        if [ -f "$PIDFILE" ]; then
          PID=$(cat "$PIDFILE" 2>/dev/null)
        fi
        if [ -n "$PID" ] && is_zapret_pid "$PID"; then
          break
        fi
        sleep 1
      done
      if [ -z "$PID" ] || ! is_zapret_pid "$PID"; then
        for p in $(pgrep -f nfqws2 2>/dev/null) $(pgrep -f nfqws 2>/dev/null); do
          if is_zapret_pid "$p"; then
            PID="$p"
            echo "$PID" > "$PIDFILE" 2>/dev/null
            break
          fi
        done
      fi
      if [ -n "$PID" ] && is_zapret_pid "$PID"; then
        add_fw
        start_failsafe
        release_zapret_lock
        exit 0
      fi
      logger -t zapret "ERROR: nfqws failed to start with args: $NFQWS_ARGS"
      echo "ERROR: nfqws failed to start with args: $NFQWS_ARGS" >&2
      stop_nfqws
      remove_fw_rules
      release_zapret_lock
      exit 1
    else
      logger -t zapret "ERROR: nfqws binary not found or not executable"
      echo "ERROR: nfqws binary not found or not executable" >&2
      release_zapret_lock
      exit 1
    fi
    ;;
  stop)
    acquire_zapret_lock || exit 1
    stop_nfqws
    remove_fw_rules
    release_zapret_lock
    exit 0
    ;;
  restart)
    acquire_zapret_lock || exit 1
    stop_nfqws
    remove_fw_rules
    sleep 1
    release_zapret_lock
    exec "$0" start
    ;;
  reload|reload-hosts)
    acquire_zapret_lock || exit 1
    PID=""
    if [ -f "$PIDFILE" ]; then
      PID=$(cat "$PIDFILE" 2>/dev/null)
    fi
    if [ -z "$PID" ] || ! is_zapret_pid "$PID"; then
      for p in $(pgrep -f nfqws2 2>/dev/null) $(pgrep -f nfqws 2>/dev/null); do
        if is_zapret_pid "$p"; then
          PID="$p"
          echo "$PID" > "$PIDFILE" 2>/dev/null
          break
        fi
      done
    fi
    if [ -n "$PID" ] && is_zapret_pid "$PID"; then
      case "$NFQWS_ARGS" in
        *zapret-hosts.txt*)
          if [ -r "/proc/$PID/cmdline" ] && ! tr '\0' ' ' < "/proc/$PID/cmdline" 2>/dev/null | grep -q "zapret-hosts.txt"; then
            release_zapret_lock
            exec "$0" restart
          fi
          ;;
      esac
      kill -HUP "$PID" 2>/dev/null
      release_zapret_lock
      exit 0
    fi
    for p in $(pgrep -f nfqws2 2>/dev/null) $(pgrep -f nfqws 2>/dev/null); do
      if is_zapret_pid "$p"; then
        kill -HUP "$p" 2>/dev/null
      fi
    done
    release_zapret_lock
    exit 0
    ;;
  start-fw|reload-fw)
    acquire_zapret_lock || exit 1
    add_fw
    ret=$?
    release_zapret_lock
    exit $ret
    ;;
  stop-fw)
    acquire_zapret_lock || exit 1
    remove_fw_rules
    release_zapret_lock
    exit 0
    ;;
  status)
    if [ -f "$PIDFILE" ]; then
      PID=$(cat "$PIDFILE" 2>/dev/null)
      if [ -n "$PID" ] && is_zapret_pid "$PID"; then
        exit 0
      fi
    fi
    for p in $(pgrep -f nfqws2 2>/dev/null) $(pgrep -f nfqws 2>/dev/null); do
      if is_zapret_pid "$p"; then
        exit 0
      fi
    done
    exit 1
    ;;
  *)
    echo "Usage: $0 {start|stop|restart|reload|start-fw|stop-fw|status}"
    exit 1
    ;;
esac
"#;

pub fn is_nfqws2_available() -> bool {
    (std::path::Path::new("/opt/zapret2/nfqws2").exists()
        || std::path::Path::new("/opt/sbin/nfqws2").exists()
        || std::path::Path::new("/opt/bin/nfqws2").exists()
        || std::path::Path::new("/opt/usr/bin/nfqws2").exists()
        || std::path::Path::new("/opt/zapret/nfq/nfqws2").exists()
        || std::path::Path::new("/opt/zapret2/binaries/arm64/nfqws2").exists()
        || std::path::Path::new("/opt/zapret2/binaries/arm/nfqws2").exists()
        || std::path::Path::new("/opt/zapret2/binaries/mipsel/nfqws2").exists()
        || std::path::Path::new("/opt/zapret2/binaries/mips/nfqws2").exists()
        || std::path::Path::new("/opt/zapret2/binaries/x86_64/nfqws2").exists()
        || std::path::Path::new("/opt/zapret2/binaries/linux-arm64/nfqws2").exists()
        || std::path::Path::new("/opt/zapret2/binaries/linux-arm/nfqws2").exists()
        || std::path::Path::new("/opt/zapret2/binaries/linux-mips32r2-lsb/nfqws2").exists()
        || std::path::Path::new("/opt/zapret2/binaries/linux-mips32r2-msb/nfqws2").exists()
        || std::path::Path::new("/opt/zapret2/binaries/linux-x86_64/nfqws2").exists())
        && (std::path::Path::new("/opt/zapret2/lua/zapret-lib.lua").exists()
            || std::path::Path::new("/opt/zapret/lua/zapret-lib.lua").exists()
            || std::path::Path::new("/opt/zapret2/files/lua/zapret-lib.lua").exists())
}

pub fn is_nfqws1_available() -> bool {
    std::path::Path::new("/opt/zapret/nfq/nfqws").exists()
        || std::path::Path::new("/opt/zapret/binaries/linux-arm64/nfqws").exists()
        || std::path::Path::new("/opt/zapret/binaries/linux-arm/nfqws").exists()
        || std::path::Path::new("/opt/zapret/binaries/linux-mips32r2-lsb/nfqws").exists()
        || std::path::Path::new("/opt/zapret/binaries/linux-mips32r2-msb/nfqws").exists()
        || std::path::Path::new("/opt/zapret/binaries/linux-x86_64/nfqws").exists()
        || std::path::Path::new("/opt/sbin/nfqws").exists()
        || std::path::Path::new("/opt/bin/nfqws").exists()
        || std::path::Path::new("/opt/usr/bin/nfqws").exists()
        || std::path::Path::new("/opt/zapret/nfqws.bak").exists()
}

pub fn normalize_engine_choice(engine: &str) -> &'static str {
    match engine.trim().to_ascii_lowercase().as_str() {
        "v1" | "legacy" | "1" | "1.x" | "nfqws" => "v1",
        _ => "v2",
    }
}

pub fn should_use_nfqws2(cfg: &crate::config::ZapretConfig) -> bool {
    #[cfg(not(target_os = "linux"))]
    {
        normalize_engine_choice(&cfg.engine) == "v2"
    }
    #[cfg(target_os = "linux")]
    {
        if normalize_engine_choice(&cfg.engine) == "v1" {
            !is_nfqws1_available() && is_nfqws2_available()
        } else {
            is_nfqws2_available()
        }
    }
}

pub async fn detect_installed_zapret1_version() -> String {
    if let Ok(ver) = tokio::fs::read_to_string("/opt/zapret/version.txt").await {
        let v = ver.trim();
        if !v.is_empty() {
            return if v.starts_with('v') { v.to_string() } else { format!("v{v}") };
        }
    }
    "v72.13".to_string()
}

pub async fn detect_installed_zapret2_version() -> String {
    if let Ok(ver) = tokio::fs::read_to_string("/opt/zapret2/version.txt").await {
        let v = ver.trim();
        if !v.is_empty() {
            return if v.starts_with('v') { v.to_string() } else { format!("v{v}") };
        }
    }
    "v1.0.6".to_string()
}

pub fn is_zapret_5am_due(last_checked: Option<&str>, now: chrono::DateTime<chrono::Local>) -> bool {
    let naive_now = now.naive_local();
    use chrono::Datelike;
    if naive_now.year() < 2024 {
        return false;
    }
    let today_5am = match naive_now.date().and_hms_opt(5, 0, 0) {
        Some(t) => t,
        None => return true,
    };
    let most_recent_5am = if naive_now >= today_5am {
        today_5am
    } else {
        today_5am - chrono::Duration::days(1)
    };

    match last_checked {
        None => true,
        Some(s) => {
            if let Ok(last_dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
                last_dt < most_recent_5am
            } else {
                true
            }
        }
    }
}

pub async fn check_zapret_update_core(state: &AppState, force: bool) -> Result<serde_json::Value, String> {
    let now = chrono::Local::now();
    let v1_inst = is_nfqws1_available();
    let v2_inst = is_nfqws2_available();
    let (engine, cur_ver, last_checked, update_avail, latest_ver) = {
        let cfg = state.config.read().await;
        let eng = if normalize_engine_choice(&cfg.zapret.engine) == "v1" {
            "v1"
        } else if v2_inst {
            "v2"
        } else if v1_inst {
            "v1"
        } else {
            normalize_engine_choice(&cfg.zapret.engine)
        };
        (
            eng.to_string(),
            if eng == "v2" { detect_installed_zapret2_version().await } else { detect_installed_zapret1_version().await },
            cfg.zapret.last_update_check.clone(),
            cfg.zapret.update_available,
            cfg.zapret.latest_version.clone(),
        )
    };

    let default_notes = if engine == "v2" {
        "• Движок Zapret 2 (nfqws2 + Lua): поддержка адаптивных Lua-стратегий десинхронизации (multisplit, multidisorder, fake TLS ClientHello с рандомизацией Session ID и tcp_ts_up).\n• Полная совместимость с YouTube, Discord и голосовыми UDP-каналами (STUN / IP Discovery).\n• Мгновенное переключение между движками Zapret 1 и Zapret 2 без переустановки."
    } else {
        "• Классический движок Zapret 1 (nfqws v72.13): проверенная десинхронизация fake,split2 / disorder2 с минимальным потреблением RAM.\n• Доступно переключение на движок Zapret 2 (nfqws2 + Lua) в 1 клик."
    };

    let is_due = is_zapret_5am_due(last_checked.as_deref(), now);

    if !force && !is_due && last_checked.is_some() {
        let label = if engine == "v2" {
            format!("Zapret 2 {cur_ver}")
        } else {
            format!("Zapret 1 {cur_ver}")
        };
        let lat = latest_ver.unwrap_or_else(|| cur_ver.clone());
        let effective_update = update_avail && crate::updater::is_newer(&lat, &cur_ver);
        return Ok(json!({
            "current_engine": engine,
            "current_version": cur_ver,
            "label": label,
            "latest_version": lat,
            "update_available": effective_update,
            "upgrade_available": engine == "v1" && !v2_inst,
            "v1_installed": v1_inst,
            "v2_installed": v2_inst,
            "notes": default_notes,
            "last_check": last_checked,
        }));
    }

    let proxy_url = {
        let cfg = state.config.read().await;
        cfg.mihomo_proxy_url()
    };

    let target_repo = if engine == "v2" {
        "bol-van/zapret2"
    } else {
        "bol-van/zapret"
    };

    let mut fetched_latest: Option<String> = None;
    let mut fetched_notes: Option<String> = None;
    let direct_client = &state.http;
    let proxied_client = reqwest::Proxy::all(&proxy_url)
        .ok()
        .and_then(|p| reqwest::Client::builder().proxy(p).build().ok());

    let mut clients = Vec::new();
    if let Some(ref p) = proxied_client {
        clients.push(p);
    }
    clients.push(direct_client);

    let gh_latest_url = format!("https://github.com/{target_repo}/releases/latest");
    for client in &clients {
        if let Ok(res) = client.get(&gh_latest_url).timeout(std::time::Duration::from_secs(6)).send().await {
            let final_url = res.url().to_string();
            if let Some(tag) = final_url.split("/releases/tag/").nth(1) {
                let tag = tag.trim_matches('/').to_string();
                if !tag.is_empty() {
                    fetched_latest = Some(tag);
                    break;
                }
            }
        }
    }

    let gh_api_url = format!("https://api.github.com/repos/{target_repo}/releases/latest");
    for client in &clients {
        if let Ok(res) = client
            .get(&gh_api_url)
            .header("User-Agent", "xkeen-route")
            .header("Accept", "application/vnd.github+json")
            .timeout(std::time::Duration::from_secs(6))
            .send()
            .await
        {
            if res.status().is_success() {
                if let Ok(body) = res.json::<serde_json::Value>().await {
                    if fetched_latest.is_none() {
                        if let Some(tag) = body.get("tag_name").and_then(|v| v.as_str()) {
                            fetched_latest = Some(tag.to_string());
                        }
                    }
                    if let Some(b) = body.get("body").and_then(|v| v.as_str()) {
                        if !b.trim().is_empty() {
                            fetched_notes = Some(b.trim().to_string());
                        }
                    }
                    break;
                }
            }
        }
    }

    let default_latest = if engine == "v2" { "v1.0.6".to_string() } else { "v72.13".to_string() };
    // Z-07: при недоступности GitHub захардкоженный дефолт НЕ считается «последней
    // версией» — иначе в офлайне появлялось фантомное update_available=true,
    // которое ещё и персистилось в конфиг. Сохраняем предыдущее состояние флага.
    let fetched_any = fetched_latest.is_some();
    let final_latest = fetched_latest.unwrap_or_else(|| latest_ver.clone().unwrap_or(default_latest));

    let has_newer_tag = fetched_any && crate::updater::is_newer(&final_latest, &cur_ver);
    let upgrade_available = engine == "v1" && !v2_inst;
    let update_available = if fetched_any { has_newer_tag } else { update_avail };

    let now_str = now.format("%Y-%m-%d %H:%M:%S").to_string();
    let display_latest = if upgrade_available && !has_newer_tag {
        "v1.0.6".to_string()
    } else {
        final_latest
    };

    {
        let _guard = state.config_lock.lock().await;
        let mut cfg = (**state.config.read().await).clone();
        cfg.zapret.last_update_check = Some(now_str.clone());
        cfg.zapret.update_available = update_available;
        cfg.zapret.latest_version = Some(display_latest.clone());
        let _ = crate::config::save(&state.config_path, &cfg).await;
        *state.config.write().await = std::sync::Arc::new(cfg);
    }

    let label = if engine == "v2" {
        format!("Zapret 2 {cur_ver}")
    } else {
        format!("Zapret 1 {cur_ver}")
    };

    Ok(json!({
        "current_engine": engine,
        "current_version": cur_ver,
        "label": label,
        "latest_version": display_latest,
        "update_available": update_available,
        "upgrade_available": upgrade_available,
        "v1_installed": v1_inst,
        "v2_installed": v2_inst,
        "notes": fetched_notes.unwrap_or_else(|| default_notes.to_string()),
        "last_check": now_str,
    }))
}

pub async fn check_zapret_update_route(
    State(state): State<AppState>,
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let force = query.get("force").map(|v| v == "1" || v == "true").unwrap_or(false);
    match check_zapret_update_core(&state, force).await {
        Ok(data) => api_ok(data),
        Err(e) => api_err(e),
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ZapretHardwareInfo {
    pub model: String,
    pub arch_label: String,
    pub target_arch: String,
    pub ram_mb: u64,
    pub recommended_engine: String,
    pub hint_text: String,
}

pub fn normalize_router_model(raw_model: &str, raw_device: &str, raw_hw_id: &str) -> String {
    let mut m = raw_model.trim().to_string();
    if m.is_empty() {
        m = raw_device.trim().to_string();
    }
    if let Some(rest) = m.strip_prefix("Keenetic ") {
        m = rest.trim().to_string();
    } else if let Some(rest) = m.strip_prefix("keenetic ") {
        m = rest.trim().to_string();
    }

    let dev = raw_device
        .trim()
        .strip_prefix("Keenetic ")
        .or_else(|| raw_device.trim().strip_prefix("keenetic "))
        .unwrap_or(raw_device.trim())
        .trim();

    if m.starts_with("KN-") && !dev.is_empty() && !dev.starts_with("KN-") {
        m = format!("{dev} {m}");
    }

    // Replace "(KN-XXXX)" with "KN-XXXX"
    if m.contains("(KN-") {
        m = m.replace("(KN-", "KN-").replace(')', "");
    }

    let hw = raw_hw_id.trim();
    if !hw.is_empty() && !m.contains(hw) {
        if m.is_empty() {
            m = hw.to_string();
        } else {
            m = format!("{m} {hw}");
        }
    }

    let cleaned = m.split_whitespace().collect::<Vec<_>>().join(" ");
    if cleaned.is_empty() {
        "Keenetic Router".to_string()
    } else {
        cleaned
    }
}

pub fn parse_arch_info(uname_m: &str, is_little_endian: bool) -> (String, String) {
    let mut arch = uname_m.trim().to_ascii_lowercase();
    if arch.is_empty() {
        arch = std::env::consts::ARCH.to_ascii_lowercase();
    }
    if arch == "aarch64" || arch == "arm64" {
        ("ARM64".to_string(), "linux-arm64".to_string())
    } else if arch.starts_with("armv7") || arch.starts_with("armv8") || arch.starts_with("arm") {
        ("ARMv7".to_string(), "linux-arm".to_string())
    } else if arch.starts_with("mips") {
        if arch.contains("el") || is_little_endian {
            ("MIPS32 (mipsel)".to_string(), "linux-mips32r2-lsb".to_string())
        } else {
            ("MIPS32 (mips)".to_string(), "linux-mips32r2-msb".to_string())
        }
    } else if arch == "x86_64" || arch == "amd64" {
        ("x86_64".to_string(), "linux-x86_64".to_string())
    } else if arch == "i686" || arch == "i386" || arch == "x86" {
        ("x86".to_string(), "linux-x86".to_string())
    } else {
        ("ARM64".to_string(), "linux-arm64".to_string())
    }
}

pub fn normalize_ram_mb(mem_total_kb: u64) -> u64 {
    let raw_mb = mem_total_kb / 1024;
    match raw_mb {
        0 => 512,
        1..=40 => 32,
        41..=72 => 64,
        73..=160 => 128,
        161..=320 => 256,
        321..=640 => 512,
        641..=1280 => 1024,
        1281..=2560 => 2048,
        other => other,
    }
}

pub fn build_zapret_hardware_info(
    raw_model: &str,
    raw_device: &str,
    raw_hw_id: &str,
    uname_m: &str,
    is_little_endian: bool,
    mem_total_kb: u64,
) -> ZapretHardwareInfo {
    let model = normalize_router_model(raw_model, raw_device, raw_hw_id);
    let (arch_label, target_arch) = parse_arch_info(uname_m, is_little_endian);
    let ram_mb = normalize_ram_mb(mem_total_kb);
    let recommended_engine = if ram_mb <= 64 || (ram_mb < 128 && target_arch.starts_with("linux-mips")) {
        "v1".to_string()
    } else {
        "v2".to_string()
    };
    let rec_label = if recommended_engine == "v2" {
        "рекомендуется Zapret 2.0"
    } else {
        "рекомендуется Legacy 1.x (экономия RAM)"
    };
    let hint_text = format!(
        "Обнаружен {} ({}, {}MB RAM) — {}",
        model, arch_label, ram_mb, rec_label
    );

    ZapretHardwareInfo {
        model,
        arch_label,
        target_arch,
        ram_mb,
        recommended_engine,
        hint_text,
    }
}

pub async fn detect_zapret_hardware(state: &AppState) -> ZapretHardwareInfo {
    let cfg = state.config.read().await.clone();
    let mut raw_model = String::new();
    let mut raw_device = String::new();
    let mut raw_hw_id = String::new();

    if let Ok(ver) = crate::rci::get_version(&state.http, &cfg).await {
        if let Some(m) = ver.get("model") {
            raw_model = m.clone();
        }
        if let Some(d) = ver.get("device") {
            raw_device = d.clone();
        }
        if let Some(h) = ver.get("hw_id") {
            raw_hw_id = h.clone();
        }
    }

    let probe_out = tokio::process::Command::new("sh")
        .arg("-c")
        .arg("cat /proc/device-tree/model 2>/dev/null || cat /tmp/sysinfo/model 2>/dev/null || awk -F: '/machine/ {print $2; exit}' /proc/cpuinfo 2>/dev/null; echo '---'; uname -m 2>/dev/null; echo '---'; awk '/MemTotal/ {print $2}' /proc/meminfo 2>/dev/null; echo '---'; (echo -n I | hexdump -o 2>/dev/null | awk '{ print substr($2,6,1); exit }' || hexdump -s 5 -n 1 -e '\"%02x\"' /bin/sh 2>/dev/null)")
        .output()
        .await
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();

    let parts: Vec<&str> = probe_out.split("---").collect();
    let probed_model = parts.first().map(|s| s.trim().trim_matches('\0')).unwrap_or("");
    if raw_model.is_empty() && !probed_model.is_empty() {
        raw_model = probed_model.to_string();
    }
    let uname_m = parts.get(1).map(|s| s.trim()).filter(|s| !s.is_empty()).unwrap_or(std::env::consts::ARCH);
    let mem_total_kb = parts
        .get(2)
        .and_then(|s| s.trim().lines().next())
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(512 * 1024);
    let endian_str = parts.get(3).map(|s| s.trim()).unwrap_or("");
    let is_little_endian = if endian_str.starts_with('1') || endian_str.starts_with("01") {
        true
    } else { endian_str.is_empty() };

    build_zapret_hardware_info(
        &raw_model,
        &raw_device,
        &raw_hw_id,
        uname_m,
        is_little_endian,
        mem_total_kb,
    )
}


pub fn convert_lua_to_legacy_desync(args: &str) -> String {
    if (args.contains("--daemon") || args.contains("--qnum"))
        && !args.contains("lua-desync")
        && !args.contains("payload=")
        && !args.contains("out-range=")
    {
        return args.replace("multisplit", "split2").replace("multidisorder", "disorder2");
    }
    if args.contains("disorder2") || args.contains("multidisorder") {
        if args.contains("midsld") {
            "--dpi-desync=fake,disorder2 --dpi-desync-split-pos=1,midsld --dpi-desync-repeats=5 --dpi-desync-fooling=ts --dpi-desync-cutoff=d4".to_string()
        } else {
            "--dpi-desync=fake,disorder2 --dpi-desync-split-pos=1 --dpi-desync-repeats=4 --dpi-desync-fooling=md5sig --dpi-desync-cutoff=d4".to_string()
        }
    } else if args.contains("seqovl") {
        "--dpi-desync=fake,split2 --dpi-desync-split-pos=1,midsld --dpi-desync-split-seqovl=5 --dpi-desync-fooling=badseq --dpi-desync-cutoff=d4".to_string()
    } else if args.contains("multisplit") {
        if args.contains("midsld") {
            "--dpi-desync=fake,split2 --dpi-desync-split-pos=1,midsld --dpi-desync-repeats=6 --dpi-desync-fooling=ts,md5sig --dpi-desync-cutoff=d4".to_string()
        } else {
            "--dpi-desync=fake,split2 --dpi-desync-split-pos=1 --dpi-desync-repeats=6 --dpi-desync-fooling=ts --dpi-desync-cutoff=d4".to_string()
        }
    } else if args.contains("split2") || args.contains("split") {
        if args.contains("ts_up") || args.contains("repeats=6") {
            "--dpi-desync=fake,split2 --dpi-desync-split-pos=1 --dpi-desync-repeats=6 --dpi-desync-fooling=ts --dpi-desync-cutoff=d4".to_string()
        } else {
            "--dpi-desync=fake,split2 --dpi-desync-split-pos=1 --dpi-desync-cutoff=d4".to_string()
        }
    } else {
        "--dpi-desync=fake,split2 --dpi-desync-split-pos=1 --dpi-desync-repeats=6 --dpi-desync-fooling=ts --dpi-desync-cutoff=d4".to_string()
    }
}

pub fn build_nfqws_args_with_desync(cfg: &crate::config::ZapretConfig, custom_desync: Option<&str>) -> (String, bool) {
    let mut profiles: Vec<String> = Vec::new();

    let default_yt_desync = if cfg.aggressive_dpi {
        "--dpi-desync=fake,split2 --dpi-desync-split-pos=1,midsld --dpi-desync-repeats=6 --dpi-desync-fooling=ts,md5sig --dpi-desync-cutoff=d4"
    } else {
        "--dpi-desync=fake,split2 --dpi-desync-split-pos=1 --dpi-desync-repeats=6 --dpi-desync-fooling=ts --dpi-desync-cutoff=d4"
    };
    let yt_desync = custom_desync.unwrap_or(default_yt_desync);

    // YouTube profile (TCP 80/443) - fake,split2 at pos 1 with repeats=6 and ts (TCP timestamp) fooling reliably bypasses TSPU inspection
    if cfg.youtube_turbo || cfg.hybrid_youtube || cfg.smart_tv_mode {
        let yt_domains = if cfg.smart_tv_mode {
            "googlevideo.com,youtube.com,ytimg.com,ggpht.com,youtu.be,yt.be,youtube-nocookie.com,redirector.googlevideo.com,manifest.googlevideo.com,gvt1.com,play.google.com"
        } else {
            "googlevideo.com,youtube.com,ytimg.com,ggpht.com,youtu.be,yt.be,youtube-nocookie.com"
        };
        profiles.push(format!(
            "--filter-tcp=80,443 --hostlist-domains={yt_domains} {yt_desync}"
        ));
    }

    // Discord Web/Chat profile - fake,split2 with ts fooling for TLS 1.3
    if cfg.hybrid_discord {
        let dc_default = if cfg.aggressive_dpi {
            "--dpi-desync=fake,split2 --dpi-desync-split-pos=1,midsld --dpi-desync-repeats=6 --dpi-desync-fooling=ts,md5sig --dpi-desync-cutoff=d4"
        } else {
            "--dpi-desync=fake,split2 --dpi-desync-split-pos=1 --dpi-desync-repeats=6 --dpi-desync-fooling=ts --dpi-desync-cutoff=d4"
        };
        let dc_desync = custom_desync.unwrap_or(dc_default);
        profiles.push(format!(
            "--filter-tcp=80,443 --hostlist-domains=discord.com,discord.gg,discordapp.com,discordapp.net,discord.media,discord-attachments-uploads-prd.storage.googleapis.com,dis.gd,discord-activities.com {dc_desync}"
        ));
    }

    // Discord Voice UDP profile
    if cfg.discord_voice_udp {
        profiles.push("--filter-udp=50000-65535 --filter-l7=discord,stun --dpi-desync=fake --dpi-desync-repeats=6".to_string());
    }

    // General Web Hostlist profile (включает универсальный хостлист, GitHub, торренты, 18+ и пользовательские сайты)
    let has_active_custom = cfg.custom_entries.iter().any(|e| e.enabled);
    if cfg.general_bypass || cfg.bypass_github || cfg.bypass_torrents || cfg.bypass_adult || cfg.community_hostlist_enabled || has_active_custom {
        let gen_default = if cfg.aggressive_dpi {
            "--dpi-desync=fake,split2 --dpi-desync-split-pos=1,midsld --dpi-desync-repeats=6 --dpi-desync-fooling=ts,md5sig --dpi-desync-cutoff=d4"
        } else {
            "--dpi-desync=fake,split2 --dpi-desync-split-pos=1 --dpi-desync-repeats=6 --dpi-desync-fooling=ts --dpi-desync-cutoff=d4"
        };
        let gen_desync = custom_desync.unwrap_or(gen_default);
        profiles.push(format!(
            "--filter-tcp=80,443 --hostlist=/opt/etc/zapret/zapret-hosts.txt {gen_desync}"
        ));
    }

    // If no specific profiles enabled, provide safe basic profile
    if profiles.is_empty() {
        let fallback_desync = custom_desync.unwrap_or("--dpi-desync=fake,split2 --dpi-desync-split-pos=1 --dpi-desync-repeats=6 --dpi-desync-fooling=ts --dpi-desync-cutoff=d4");
        profiles.push(format!("--filter-tcp=80,443 --hostlist-domains=googlevideo.com,youtube.com,ytimg.com,ggpht.com,youtu.be,yt.be,youtube-nocookie.com,discord.com,discord.gg,discordapp.com {}", fallback_desync));
    }

    let args = format!("--daemon --qnum=200 --dpi-desync-fwmark=0x40000000 {}", profiles.join(" --new "));
    let voice_enabled = cfg.discord_voice_udp;
    (args, voice_enabled)
}

pub fn build_nfqws_args(cfg: &crate::config::ZapretConfig) -> (String, bool) {
    build_nfqws_args_with_desync(cfg, None)
}

/// Генератор аргументов zapret2 (nfqws2 с рантаймом Lua, --payload и --lua-desync)
pub fn build_nfqws2_args_with_desync(cfg: &crate::config::ZapretConfig, custom_desync: Option<&str>) -> (String, bool) {
    let mut profiles: Vec<String> = Vec::new();

    let default_fake_desync = if cfg.aggressive_dpi {
        "--lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=6:tls_mod=rnd,dupsid:tcp_ts_up:seqovl=5:tcp_ack=-66000 --lua-desync=multisplit:pos=1,midsld"
    } else if cfg.youtube_turbo || cfg.smart_tv_mode {
        "--lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=6:tls_mod=rnd,dupsid:tcp_ts_up --lua-desync=multisplit:pos=1,midsld"
    } else {
        "--lua-desync=fake:blob=fake_default_tls --lua-desync=multisplit:pos=1"
    };
    let fake_desync = custom_desync.unwrap_or(default_fake_desync);

    // YouTube profile (TCP 80/443) - Fake ClientHello + multisplit desync с рандомизацией и ранняя отсечка -d10
    if cfg.youtube_turbo || cfg.hybrid_youtube || cfg.smart_tv_mode {
        let yt_domains = if cfg.smart_tv_mode {
            "googlevideo.com,youtube.com,ytimg.com,ggpht.com,youtu.be,yt.be,youtube-nocookie.com,redirector.googlevideo.com,manifest.googlevideo.com,gvt1.com,play.google.com"
        } else {
            "googlevideo.com,youtube.com,ytimg.com,ggpht.com,youtu.be,yt.be,youtube-nocookie.com"
        };
        profiles.push(format!(
            "--filter-tcp=80,443 --filter-l7=tls,http --hostlist-domains={yt_domains} --out-range=-d10 --payload=tls_client_hello {fake_desync}"
        ));
    }

    // Discord Web/Chat profile (TCP 80/443)
    if cfg.hybrid_discord {
        let dc_default = if cfg.aggressive_dpi {
            "--lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=6:tls_mod=rnd,dupsid:tcp_ts_up:seqovl=5:tcp_ack=-66000 --lua-desync=multisplit:pos=1,midsld"
        } else {
            "--lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=6:tls_mod=rnd,dupsid:tcp_ts_up --lua-desync=multisplit:pos=1,midsld"
        };
        let dc_desync = custom_desync.unwrap_or(dc_default);
        profiles.push(format!(
            "--filter-tcp=80,443 --filter-l7=tls,http --hostlist-domains=discord.com,discord.gg,discordapp.com,discordapp.net,discord.media,discord-attachments-uploads-prd.storage.googleapis.com,dis.gd,discord-activities.com --out-range=-d10 --payload=tls_client_hello {dc_desync}"
        ));
    }

    // Discord Voice UDP profile (перехват голосовых каналов UDP 50000:65535, фейковые пакеты для STUN и IP Discovery)
    if cfg.discord_voice_udp {
        profiles.push("--filter-udp=50000-65535 --filter-l7=discord,stun --payload=discord_ip_discovery,stun --lua-desync=fake:blob=0x00000000:repeats=4".to_string());
    }

    // General Web Hostlist profile (общий файл zapret-hosts.txt)
    let has_active_custom = cfg.custom_entries.iter().any(|e| e.enabled);
    if cfg.general_bypass || cfg.bypass_github || cfg.bypass_torrents || cfg.bypass_adult || cfg.community_hostlist_enabled || has_active_custom {
        let gen_default = if cfg.aggressive_dpi {
            "--lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=6:tls_mod=rnd,dupsid:tcp_ts_up:seqovl=5:tcp_ack=-66000 --lua-desync=multisplit:pos=1,midsld"
        } else {
            "--lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=6:tls_mod=rnd,dupsid:tcp_ts_up --lua-desync=multisplit:pos=1"
        };
        let desync_args = custom_desync.unwrap_or(gen_default);
        profiles.push(format!(
            "--filter-tcp=80,443 --filter-l7=tls,http --hostlist=/opt/etc/zapret/zapret-hosts.txt --out-range=-d10 --payload=tls_client_hello {desync_args}"
        ));
    }

    // Базовый безопасный профиль по умолчанию, если ничего не выбрано
    if profiles.is_empty() {
        let fallback_desync = custom_desync.unwrap_or("--lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=6:tls_mod=rnd,dupsid:tcp_ts_up --lua-desync=multisplit:pos=1");
        profiles.push(format!("--filter-tcp=80,443 --filter-l7=tls,http --hostlist-domains=googlevideo.com,youtube.com,ytimg.com,ggpht.com,youtu.be,yt.be,youtube-nocookie.com,discord.com,discord.gg,discordapp.com --out-range=-d10 --payload=tls_client_hello {}", fallback_desync));
    }

    let args = format!("--daemon --qnum=200 --dpi-desync-fwmark=0x40000000 {}", profiles.join(" --new "));
    let voice_enabled = cfg.discord_voice_udp;
    (args, voice_enabled)
}

pub fn build_nfqws2_args(cfg: &crate::config::ZapretConfig) -> (String, bool) {
    build_nfqws2_args_with_desync(cfg, None)
}

pub const ZAPRET_HOSTS_MANAGED_BEGIN: &str = "# --- START XKEEN ROUTE DYNAMIC HOSTS ---";
pub const ZAPRET_HOSTS_MANAGED_END: &str = "# --- END XKEEN ROUTE DYNAMIC HOSTS ---";
pub const ZAPRET_COMMUNITY_MANAGED_BEGIN: &str = "# --- START COMMUNITY HOSTLIST ---";
pub const ZAPRET_COMMUNITY_MANAGED_END: &str = "# --- END COMMUNITY HOSTLIST ---";

pub fn strip_managed_blocks(text: &str) -> String {
    let mut manual_lines: Vec<String> = Vec::new();
    let mut in_managed_block = false;
    let mut in_community_block = false;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed == ZAPRET_HOSTS_MANAGED_BEGIN {
            in_managed_block = true;
            continue;
        }
        if trimmed == ZAPRET_HOSTS_MANAGED_END {
            in_managed_block = false;
            continue;
        }
        if trimmed == ZAPRET_COMMUNITY_MANAGED_BEGIN {
            in_community_block = true;
            continue;
        }
        if trimmed == ZAPRET_COMMUNITY_MANAGED_END {
            in_community_block = false;
            continue;
        }
        if !in_managed_block && !in_community_block {
            manual_lines.push(line.to_string());
        }
    }
    manual_lines.join("\n")
}

pub fn sync_zapret_hosts_content(existing: &str, cfg: &crate::config::ZapretConfig) -> String {
    // 1. Исключаем существующий блок авто-управления и community-блок, сохраняя ручные строки пользователя
    let stripped = strip_managed_blocks(existing);
    let mut manual_lines: Vec<String> = stripped.lines().map(|l| l.to_string()).collect();

    // 2. Список динамически управляемых доменов
    let mut dynamic_domains: Vec<String> = Vec::new();

    let mut add_dynamic = |d: &str| {
        let clean = crate::config::normalize_domain(d);
        if !clean.is_empty() && !dynamic_domains.iter().any(|existing| existing.eq_ignore_ascii_case(&clean)) {
            dynamic_domains.push(clean);
        }
    };

    if cfg.bypass_github {
        for d in crate::routing::GITHUB_DOMAINS {
            add_dynamic(d);
        }
    }

    if cfg.bypass_torrents {
        for d in crate::routing::TORRENT_DOMAINS {
            add_dynamic(d);
        }
    }

    if cfg.bypass_adult {
        for d in crate::routing::ADULT_DOMAINS {
            add_dynamic(d);
        }
    }

    for entry in &cfg.custom_entries {
        if entry.enabled {
            add_dynamic(&entry.domain);
            for cdn in &entry.cdns {
                add_dynamic(cdn);
            }
        }
    }

    // 3. Удаляем динамические домены из ручной части (предотвращает дубли и зависание отключенных доменов)
    let mut all_managed: std::collections::HashSet<String> = std::collections::HashSet::new();
    for d in crate::routing::GITHUB_DOMAINS {
        all_managed.insert(crate::config::normalize_domain(d));
    }
    for d in crate::routing::TORRENT_DOMAINS {
        all_managed.insert(crate::config::normalize_domain(d));
    }
    for d in crate::routing::ADULT_DOMAINS {
        all_managed.insert(crate::config::normalize_domain(d));
    }
    for entry in &cfg.custom_entries {
        all_managed.insert(crate::config::normalize_domain(&entry.domain));
        for cdn in &entry.cdns {
            all_managed.insert(crate::config::normalize_domain(cdn));
        }
    }

    manual_lines.retain(|line| {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            return true;
        }
        let norm = crate::config::normalize_domain(trimmed);
        !all_managed.contains(&norm)
    });

    // 4. Сборка итогового файла
    let mut result = manual_lines.join("\n").trim().to_string();
    if !result.is_empty() {
        result.push('\n');
    }

    if !dynamic_domains.is_empty() {
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(ZAPRET_HOSTS_MANAGED_BEGIN);
        result.push_str("\n# Автоматически управляемые службы и сайты /boost (Zapret)\n");
        for d in dynamic_domains {
            result.push_str(&d);
            result.push('\n');
        }
        result.push_str(ZAPRET_HOSTS_MANAGED_END);
        result.push('\n');
    }

    result
}

pub fn validate_custom_args(args: &str) -> Result<(), String> {
    for ch in args.chars() {
        if ch == ';' || ch == '&' || ch == '|' || ch == '`' || ch == '$' || ch == '\n' || ch == '\r' || ch == '(' || ch == ')' || ch == '<' || ch == '>' || ch == '!' || ch == '"' || ch == '\'' {
            return Err(format!("Недопустимый спецсимвол в аргументах Zapret: '{}'. Разрешены только флаги nfqws/nfqws2", ch));
        }
    }
    parse_zapret_queue_spec(args).map(|_| ())?;
    Ok(())
}

pub fn validate_zapret_conf(content: &str) -> Result<(), String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((key, val)) = trimmed.split_once('=') {
            let k = key.trim();
            if k == "NFQWS_ARGS" {
                let clean_val = val.trim().trim_matches('"').trim_matches('\'');
                validate_custom_args(clean_val)?;
            } else if k == "ZAPRET_ENGINE" {
                let clean_val = val.trim().trim_matches('"').trim_matches('\'');
                if clean_val != "v1" && clean_val != "v2" && clean_val != "legacy" && clean_val != "modern" {
                    return Err(format!("Недопустимое значение ZAPRET_ENGINE: '{clean_val}'. Разрешены только v1, v2"));
                }
            } else if k == "EXCLUDED_IPS" {
                // Z-04: значения попадают в `iptables -s "$ex_ip"` внутри S51zapret —
                // без валидации command substitution `$(cmd)` выполнялся бы как RCE.
                let clean_val = val.trim().trim_matches('"').trim_matches('\'');
                for ip in clean_val.split_whitespace() {
                    if !crate::config::is_valid_ip_or_cidr(ip) {
                        return Err(format!("Недопустимое значение в EXCLUDED_IPS: '{ip}'. Разрешены только IP-адреса или CIDR"));
                    }
                }
            } else if k == "EXCLUDED_MACS" {
                // Z-04: аналогичная защита для MAC-исключений
                let clean_val = val.trim().trim_matches('"').trim_matches('\'');
                for mac in clean_val.split_whitespace() {
                    if !crate::config::is_valid_mac(mac) {
                        return Err(format!("Недопустимое значение в EXCLUDED_MACS: '{mac}'. Разрешены только MAC-адреса формата 00:11:22:33:44:55"));
                    }
                }
            } else if k != "DISCORD_VOICE_ENABLED" && k != "BLOCK_QUIC" && k != "SMART_TV_MODE" && k != "ADGUARD_ENABLED" {
                return Err(format!("Неизвестный параметр в zapret.conf: '{k}'. Разрешены только ZAPRET_ENGINE, NFQWS_ARGS, DISCORD_VOICE_ENABLED, BLOCK_QUIC, SMART_TV_MODE, EXCLUDED_IPS, EXCLUDED_MACS, ADGUARD_ENABLED"));
            }
        } else {
            return Err("Некорректная строка в zapret.conf: должна быть в формате КЛЮЧ=\"ЗНАЧЕНИЕ\"".to_string());
        }
    }
    Ok(())
}

/// Флаг ADGUARD_ENABLED для zapret.conf (баг 31): AdGuard «активен», только если интеграция включена
/// в конфиге или процесс AdGuardHome реально работает. Одно наличие init-скрипта ничего не значит.
pub fn zapret_conf_adguard_flag(integration_enabled: bool, process_running: bool, _installed: bool) -> bool {
    integration_enabled || process_running
}

pub async fn sync_zapret_files(cfg: &crate::config::ZapretConfig) -> Result<(), String> {
    sync_zapret_files_with_config_path(cfg, std::path::Path::new("/opt/etc/xkeen-route/config.json")).await
}

pub async fn sync_zapret_files_with_config_path(
    cfg: &crate::config::ZapretConfig,
    config_path: &std::path::Path,
) -> Result<(), String> {
    tokio::fs::create_dir_all("/opt/etc/zapret")
        .await
        .map_err(|e| format!("Не удалось создать /opt/etc/zapret: {e}"))?;
    tokio::fs::create_dir_all("/opt/etc/init.d")
        .await
        .map_err(|e| format!("Не удалось создать /opt/etc/init.d: {e}"))?;

    // 1. S51zapret script
    let mihomo_dns_port = std::fs::read_to_string(&crate::config::MihomoConfig::default().config_path)
        .ok()
        .and_then(|y| crate::routing::parse_mihomo_dns_listen_port(&y))
        .unwrap_or(crate::routing::DEFAULT_MIHOMO_DNS_PORT);
    let gaming_ips: Vec<String> = {
        if config_path.exists() {
            let app_cfg = crate::config::load(config_path);
            if app_cfg.gaming.enabled {
                app_cfg
                    .gaming
                    .devices
                    .iter()
                    .filter(|d| d.enabled && !d.ip.trim().is_empty())
                    .map(|d| d.ip.trim().to_string())
                    .collect()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        }
    };
    let mut queue_spec = match cfg.custom_args.as_deref() {
        Some(custom) if !custom.trim().is_empty() => {
            validate_custom_args(custom)?;
            parse_zapret_queue_spec(custom)?
        }
        _ => QueueSpec::default(),
    };
    if let Some(custom) = cfg.custom_args.as_deref() {
        let is_full_cmdline = custom.contains("--daemon") || custom.contains("--qnum");
        if !is_full_cmdline && queue_spec.tcp_ports.is_empty() {
            queue_spec.tcp_ports = vec!["80".to_string(), "443".to_string()];
        }
    }
    let s51_script = build_zapret_firewall_script_with_queue(mihomo_dns_port, &gaming_ips, &queue_spec);
    atomic_write_file("/opt/etc/init.d/S51zapret", &s51_script)
        .await
        .map_err(|e| format!("Не удалось записать /opt/etc/init.d/S51zapret: {e}"))?;
    tokio::process::Command::new("chmod")
        .arg("+x")
        .arg("/opt/etc/init.d/S51zapret")
        .output()
        .await
        .map_err(|e| format!("Не удалось сделать S51zapret исполняемым: {e}"))?;

    // 1.1 Keenetic ndm netfilter & ifstatechanged hooks (prevent netfilter drop on DHCP/WAN reload)
    let _ = tokio::fs::create_dir_all("/opt/etc/ndm/netfilter.d").await;
    let _ = tokio::fs::create_dir_all("/opt/etc/ndm/ifstatechanged.d").await;
    let _ = tokio::fs::create_dir_all("/opt/etc/ndm/wan.d").await;
    if let Ok(()) = atomic_write_file("/opt/etc/ndm/netfilter.d/050-zapret.sh", NDM_NETFILTER_SCRIPT).await {
        let _ = tokio::process::Command::new("chmod")
            .arg("+x")
            .arg("/opt/etc/ndm/netfilter.d/050-zapret.sh")
            .output()
            .await;
    }
    if let Ok(()) = atomic_write_file("/opt/etc/ndm/ifstatechanged.d/050-zapret.sh", NDM_IFSTATE_SCRIPT).await {
        let _ = tokio::process::Command::new("chmod")
            .arg("+x")
            .arg("/opt/etc/ndm/ifstatechanged.d/050-zapret.sh")
            .output()
            .await;
    }
    if let Ok(()) = atomic_write_file("/opt/etc/ndm/wan.d/050-zapret.sh", NDM_IFSTATE_SCRIPT).await {
        let _ = tokio::process::Command::new("chmod")
            .arg("+x")
            .arg("/opt/etc/ndm/wan.d/050-zapret.sh")
            .output()
            .await;
    }

    // 2. Default hostlist if missing, and sync active domains into it
    let hosts_path = "/opt/etc/zapret/zapret-hosts.txt";
    let base_hosts = if std::path::Path::new(hosts_path).exists() {
        tokio::fs::read_to_string(hosts_path).await.unwrap_or_else(|_| DEFAULT_ZAPRET_HOSTS.to_string())
    } else {
        DEFAULT_ZAPRET_HOSTS.to_string()
    };
    let mut synced_hosts = sync_zapret_hosts_content(&base_hosts, cfg);

    // Подгрузка Community Hostlist при активации
    if cfg.community_hostlist_enabled {
        if let Ok(comm) = tokio::fs::read_to_string("/opt/etc/zapret/community-hosts.txt").await {
            let comm_domains: Vec<&str> = comm
                .lines()
                .map(|l| l.trim())
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .collect();
            if !comm_domains.is_empty() {
                synced_hosts.push_str("\n");
                synced_hosts.push_str(ZAPRET_COMMUNITY_MANAGED_BEGIN);
                synced_hosts.push('\n');
                for d in comm_domains {
                    synced_hosts.push_str(d);
                    synced_hosts.push('\n');
                }
                synced_hosts.push_str(ZAPRET_COMMUNITY_MANAGED_END);
                synced_hosts.push('\n');
            }
        }
    }
    let _ = atomic_write_file(hosts_path, &synced_hosts).await;

    // 3. zapret.conf через единый сериализатор (баг 35)
    if let Some(custom) = &cfg.custom_args {
        validate_custom_args(custom)?;
    }
    let conf_data = crate::zapret::serialize_zapret_conf(cfg, config_path);
    atomic_write_file("/opt/etc/zapret/zapret.conf", &conf_data)
        .await
        .map_err(|e| format!("Не удалось записать /opt/etc/zapret/zapret.conf: {e}"))?;
    Ok(())
}

pub fn format_nfqws_proc_cmdline(raw: &[u8]) -> String {
    let mut c = String::new();
    for (idx, part) in raw.split(|&b| b == 0).filter(|s| !s.is_empty()).enumerate() {
        let token = String::from_utf8_lossy(part);
        if idx == 0 {
            c.push_str(&token);
        } else if token.starts_with('-') {
            c.push(' ');
            c.push_str(&token);
        } else {
            // Legacy nfqws modifies argv in-place via strtok(optarg, ","), replacing ',' with '\0'
            c.push(',');
            c.push_str(&token);
        }
    }
    c
}


/// GET /api/zapret/status — статус nfqws/nfqws2, iptables и S51zapret
pub async fn get_zapret_status(State(state): State<AppState>) -> Response {
    let init_script = std::path::Path::new("/opt/etc/init.d/S51zapret");
    let installed = init_script.exists() && (is_nfqws2_available() || is_nfqws1_available());

    let mut running = false;
    let mut pid: Option<u32> = None;
    let mut cmdline: Option<String> = None;

    if installed {
        if let Some(p) = crate::zapret::find_our_zapret_pid().await {
            running = true;
            pid = Some(p);
            let proc_cmd = format!("/proc/{p}/cmdline");
            if let Ok(raw) = tokio::fs::read(&proc_cmd).await {
                let c = format_nfqws_proc_cmdline(&raw);
                if !c.is_empty() {
                    cmdline = Some(c);
                }
            }
        }
    }

    let autostart = if installed {
        tokio::process::Command::new("sh")
            .arg("-c")
            .arg("[ -x /opt/etc/init.d/S51zapret ]")
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false)
    } else {
        false
    };

    let mut iptables_active = tokio::process::Command::new("sh")
        .arg("-c")
        .arg("(iptables -w 2 -t mangle -S POSTROUTING 2>/dev/null | grep -q zapret || iptables -w 2 -t mangle -S PREROUTING 2>/dev/null | grep -q zapret) && iptables -w 2 -t mangle -nL zapret 2>/dev/null | grep -q NFQUEUE")
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false);

    let config_content = tokio::fs::read_to_string("/opt/etc/zapret/zapret.conf").await.ok();
    let hosts_content = tokio::fs::read_to_string("/opt/etc/zapret/zapret-hosts.txt").await.ok();

    let cfg = state.config.read().await.clone();

    // Z-05: GET-эндпоинт стал строго read-only — убирать процессы здесь нельзя:
    // поллинг каждые 10с и мутации в идемпотентном GET — источник гонок со стартом.
    // Чисткой осиротевшего процесса при выключенной службе занимается watchdog.
    if !cfg.zapret.enabled {
        running = false;
        iptables_active = false;
        pid = None;
        cmdline = None;
    }

    // Определение текущего пресета
    let check_str = cmdline.as_deref().or(config_content.as_deref()).unwrap_or("");
    let preset = if cfg.zapret.custom_args.is_some() {
        "custom"
    } else if cfg.zapret.aggressive_dpi || check_str.contains("ts,md5sig") || check_str.contains("badseq,md5sig") {
        "aggressive"
    } else if cfg.zapret.youtube_turbo && !cfg.zapret.discord_voice_udp && !cfg.zapret.general_bypass && !cfg.zapret.bypass_github && !cfg.zapret.hybrid_discord {
        "youtube"
    } else if !cfg.zapret.youtube_turbo && (cfg.zapret.discord_voice_udp || cfg.zapret.hybrid_discord) && !cfg.zapret.general_bypass && !cfg.zapret.hybrid_youtube {
        "discord"
    } else if cfg.zapret.youtube_turbo && cfg.zapret.discord_voice_udp && cfg.zapret.general_bypass && !cfg.zapret.bypass_adult {
        "gamer"
    } else if cfg.zapret.youtube_turbo && cfg.zapret.discord_voice_udp && cfg.zapret.general_bypass && cfg.zapret.bypass_adult {
        "default"
    } else if cfg.zapret.general_bypass {
        "general"
    } else {
        "custom"
    };

    let v2_installed = is_nfqws2_available();
    let v1_installed = is_nfqws1_available();
    let can_rollback_v1 = v1_installed
        || std::path::Path::new("/opt/etc/zapret/zapret.v1.conf.bak").exists()
        || std::path::Path::new("/opt/zapret/nfqws.bak").exists();
    let active_engine = if let Some(ref cmd) = cmdline {
        if cmd.contains("nfqws2") || cmd.contains("--lua-desync") {
            "v2"
        } else {
            "v1"
        }
    } else if normalize_engine_choice(&cfg.zapret.engine) == "v1" {
        "v1"
    } else if v2_installed {
        "v2"
    } else if v1_installed {
        "v1"
    } else {
        normalize_engine_choice(&cfg.zapret.engine)
    };
    let hardware = detect_zapret_hardware(&state).await;

    let (zapret_ver, zapret_label) = if active_engine == "v2" {
        let v = detect_installed_zapret2_version().await;
        let l = format!("Zapret 2 {v}");
        (v, l)
    } else {
        let v = detect_installed_zapret1_version().await;
        let l = format!("Zapret 1 {v}");
        (v, l)
    };

    let active_strategy_id: Option<String> = if let Some(ref ca) = cfg.zapret.custom_args {
        let ca_trimmed = ca.trim();
        BLOCKCHECK_STRATEGIES
            .iter()
            .find(|(_, _, _, s_args)| {
                let s_trimmed = s_args.trim();
                ca_trimmed == s_trimmed
                    || ca_trimmed.contains(s_trimmed)
                    || ca_trimmed == convert_lua_to_legacy_desync(s_trimmed).trim()
            })
            .map(|(id, _, _, _)| (*id).to_string())
    } else if cfg.zapret.aggressive_dpi {
        Some("aggressive_dupsid".to_string())
    } else if cfg.zapret.youtube_turbo {
        Some("multisplit".to_string())
    } else {
        None
    };

    let current_op = crate::zapret::get_current_operation_state();
    let operation_in_progress = current_op != crate::zapret::ZapretOperationState::Idle;
    let mihomo_dns = crate::routing::mihomo_dns_port(&cfg);
    let dns_resolver_ready = crate::zapret::check_dns_resolver_ready(mihomo_dns).await;
    let dns_redirect_active = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(format!("iptables -w 2 -t nat -S PREROUTING 2>/dev/null | grep -q '{mihomo_dns}'"))
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false);

    api_ok(json!({
        "installed": installed,
        "running": running,
        "pid": pid,
        "autostart": autostart,
        "iptables_active": iptables_active,
        "dns_resolver_ready": dns_resolver_ready,
        "dns_redirect_active": dns_redirect_active,
        "operation_state": current_op,
        "operation_in_progress": operation_in_progress,
        "preset": preset,
        "active_strategy_id": active_strategy_id,
        "cmdline": cmdline,
        "config": config_content,
        "hosts": hosts_content,
        "features": &cfg.zapret,
        "engine": active_engine,
        "version": zapret_ver,
        "version_label": zapret_label,
        "update_available": cfg.zapret.update_available
            && cfg.zapret.latest_version.as_deref().is_some_and(|lat| crate::updater::is_newer(lat, &zapret_ver)),
        "latest_version": cfg.zapret.latest_version,
        "v2_installed": v2_installed,
        "v1_installed": v1_installed,
        "can_rollback_v1": can_rollback_v1,
        "hardware": hardware,
    }))
}

#[derive(Deserialize)]
pub struct ZapretActionReq {
    pub action: String, // "start" | "stop" | "restart" | "toggle" | "install" | "upgrade_zapret2" | "rollback_v1" | "switch_engine" | "set_preset" | "save_config" | "save_hosts" | "test_dpi" | "toggle_feature" | "set_features" | "reset_features" | "add_custom_domain" | "remove_custom_domain" | "toggle_custom_domain" | "boost_custom_domain" | "apply_strategy" | "reset_analytics" | "sync_community_hostlist"
    pub preset: Option<String>,
    pub custom_args: Option<String>,
    pub config_content: Option<String>,
    pub hosts_content: Option<String>,
    pub feature: Option<String>,
    pub enabled: Option<bool>,
    pub features: Option<ZapretFeatures>,
    pub domain: Option<String>,
    pub strategy_id: Option<String>,
    pub url: Option<String>,
    pub engine: Option<String>,
    pub version: Option<String>,
}

/// Поля «функций» Zapret, передаваемые из UI в `set_features` (без перезаписи устройств, движка и статистики).
#[derive(Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct ZapretFeatures {
    pub enabled: Option<bool>,
    pub hybrid_youtube: Option<bool>,
    pub hybrid_discord: Option<bool>,
    pub discord_voice_udp: Option<bool>,
    pub youtube_turbo: Option<bool>,
    pub general_bypass: Option<bool>,
    pub aggressive_dpi: Option<bool>,
    pub isolated_proxy: Option<bool>,
    pub bypass_github: Option<bool>,
    pub bypass_torrents: Option<bool>,
    pub bypass_adult: Option<bool>,
    pub custom_entries: Option<Vec<crate::config::ZapretCustomEntry>>,
    pub custom_args: Option<String>,
    pub smart_tv_mode: Option<bool>,
    pub community_hostlist_enabled: Option<bool>,
    pub community_hostlist_url: Option<String>,
    pub community_hostlist_auto_update: Option<bool>,
    pub excluded_devices: Option<Vec<String>>,
    pub engine: Option<String>,
}

/// Накладывает поля функций из запроса на текущий `ZapretConfig`, сохраняя
/// `excluded_devices`, `custom_entries`, `engine`, метаданные community-листа и статистику.
pub fn merge_zapret_features(
    cur: &crate::config::ZapretConfig,
    req: &ZapretFeatures,
) -> crate::config::ZapretConfig {
    let mut merged = cur.clone();
    if let Some(v) = req.enabled {
        merged.enabled = v;
    }
    if let Some(v) = req.hybrid_youtube {
        merged.hybrid_youtube = v;
    }
    if let Some(v) = req.hybrid_discord {
        merged.hybrid_discord = v;
    }
    if let Some(v) = req.discord_voice_udp {
        merged.discord_voice_udp = v;
    }
    if let Some(v) = req.youtube_turbo {
        merged.youtube_turbo = v;
    }
    if let Some(v) = req.general_bypass {
        merged.general_bypass = v;
    }
    if let Some(v) = req.aggressive_dpi {
        merged.aggressive_dpi = v;
    }
    if let Some(v) = req.isolated_proxy {
        merged.isolated_proxy = v;
    }
    if let Some(v) = req.bypass_github {
        merged.bypass_github = v;
    }
    if let Some(v) = req.bypass_torrents {
        merged.bypass_torrents = v;
    }
    if let Some(v) = req.bypass_adult {
        merged.bypass_adult = v;
    }
    if let Some(ref ca) = req.custom_args {
        merged.custom_args = if !should_use_nfqws2(&merged)
            && (ca.contains("--lua-desync") || ca.contains("multisplit"))
        {
            Some(convert_lua_to_legacy_desync(ca))
        } else {
            Some(ca.clone())
        };
    }
    if let Some(v) = req.smart_tv_mode {
        merged.smart_tv_mode = v;
    }
    if let Some(v) = req.community_hostlist_enabled {
        merged.community_hostlist_enabled = v;
    }
    if let Some(ref u) = req.community_hostlist_url {
        if !u.trim().is_empty() {
            merged.community_hostlist_url = u.trim().to_string();
        }
    }
    if let Some(v) = req.community_hostlist_auto_update {
        merged.community_hostlist_auto_update = v;
    }
    merged
}

/// Сбрасывает только поля «функций» Zapret в значения по умолчанию,
/// сохраняя `enabled`, `excluded_devices`, `custom_entries`, `engine`, метаданные и статистику.
pub fn reset_zapret_features(cur: &crate::config::ZapretConfig) -> crate::config::ZapretConfig {
    let mut reset = crate::config::ZapretConfig::default();
    reset.enabled = cur.enabled;
    reset.excluded_devices = cur.excluded_devices.clone();
    reset.custom_entries = cur.custom_entries.clone();
    reset.engine = cur.engine.clone();
    reset.community_hostlist_last_updated = cur.community_hostlist_last_updated.clone();
    reset.community_hostlist_count = cur.community_hostlist_count;
    reset.last_update_check = cur.last_update_check.clone();
    reset.update_available = cur.update_available;
    reset.latest_version = cur.latest_version.clone();
    reset.period_saved_bytes = cur.period_saved_bytes;
    reset
}

/// Точечное применение пресета Zapret без сброса несвязанных флагов пользователя (баг 111).
pub fn apply_zapret_preset(
    zcfg: &mut crate::config::ZapretConfig,
    p: &str,
    custom_args_opt: Option<&str>,
) -> Result<(), String> {
    match p {
        "youtube" => {
            zcfg.custom_args = None;
            zcfg.youtube_turbo = true;
            zcfg.hybrid_youtube = true;
        }
        "discord" => {
            zcfg.custom_args = None;
            zcfg.hybrid_discord = true;
            zcfg.discord_voice_udp = true;
        }
        "gamer" | "media" => {
            zcfg.custom_args = None;
            zcfg.youtube_turbo = true;
            zcfg.hybrid_youtube = true;
            zcfg.hybrid_discord = true;
            zcfg.discord_voice_udp = true;
            zcfg.general_bypass = true;
            zcfg.aggressive_dpi = false;
            zcfg.isolated_proxy = true;
            zcfg.bypass_github = true;
            zcfg.bypass_torrents = true;
        }
        "aggressive" => {
            zcfg.custom_args = None;
            zcfg.youtube_turbo = true;
            zcfg.hybrid_youtube = true;
            zcfg.hybrid_discord = true;
            zcfg.discord_voice_udp = true;
            zcfg.general_bypass = true;
            zcfg.aggressive_dpi = true;
            zcfg.isolated_proxy = true;
            zcfg.bypass_github = true;
            zcfg.bypass_torrents = true;
            zcfg.bypass_adult = true;
        }
        "custom" => {
            if let Some(custom) = custom_args_opt {
                let clean = custom.trim().to_string();
                if !clean.is_empty() {
                    validate_custom_args(&clean)?;
                    let effective = if !should_use_nfqws2(zcfg)
                        && (clean.contains("--lua-desync") || clean.contains("multisplit"))
                    {
                        convert_lua_to_legacy_desync(&clean)
                    } else {
                        clean
                    };
                    zcfg.custom_args = Some(effective);
                }
            }
        }
        _ => {
            // "general" / "all"
            zcfg.custom_args = None;
            zcfg.youtube_turbo = true;
            zcfg.hybrid_youtube = true;
            zcfg.hybrid_discord = true;
            zcfg.discord_voice_udp = true;
            zcfg.general_bypass = true;
            zcfg.aggressive_dpi = false;
            zcfg.isolated_proxy = true;
            zcfg.bypass_github = true;
            zcfg.bypass_torrents = true;
            zcfg.bypass_adult = true;
        }
    }
    Ok(())
}

/// Применение пресета конфигурации Zapret (используется и веб-панелью, и Telegram-ботом)
pub async fn apply_zapret_preset_action(
    state: &AppState,
    p: &str,
    custom_args_opt: Option<&str>,
) -> Result<String, String> {
    let _cfg_guard = state.config_lock.lock().await;
    let _routing_guard = state.routing_lock.lock().await;
    let mut cfg = (**state.config.read().await).clone();

    apply_zapret_preset(&mut cfg.zapret, p, custom_args_opt)?;

    sync_zapret_files_with_config_path(&cfg.zapret, &state.config_path)
        .await
        .map_err(|e| format!("Ошибка синхронизации файлов Zapret: {e}"))?;
    // X-07: set_clean_dns_servers (добавление Яндекс/Cloudflare DNS в KeeneticOS + save
    // конфига роутера) убран из пресетов — это скрытая мутация настроек роутера из
    // «косметического» действия. Чистые DNS применяются только явной кнопкой (/api/dns/clean-servers, баг 56).

    // Обновляем правила в config.yaml ядра Mihomo через apply_routing
    if std::path::Path::new(&cfg.mihomo.config_path).exists() {
        if let Ok(raw_yaml) = tokio::fs::read_to_string(&cfg.mihomo.config_path).await {
            if let Ok((new_yaml, _)) = routing::apply_routing(&raw_yaml, &cfg) {
                let _ = atomic_write_file(&cfg.mihomo.config_path, &new_yaml).await;
                let _ = mihomo::reload_config(&state.http, &cfg).await;
            }
        }
    }

    let _ = config::save(&state.config_path, &cfg).await;
    *state.config.write().await = std::sync::Arc::new(cfg.clone());

    #[cfg(unix)]
    {
        let is_running = crate::zapret::find_our_zapret_pid().await.is_some();
        if is_running && !cfg.zapret.enabled {
            let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("stop").output().await;
        } else if cfg.zapret.enabled {
            let cmd_arg = if is_running { "restart" } else { "start" };
            if let Ok(out) = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg(cmd_arg).output().await {
                if !out.status.success() {
                    let err_msg = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
                    return Err(format!("Ошибка перезапуска Zapret при применении пресета '{p}':\n{}", err_msg.trim()));
                }
            }
        }
    }

    Ok(format!("Применен пресет '{p}'"))
}

/// POST /api/zapret/action — запуск, остановка, переключение, пресеты и тест DPI
/// X-04: Ядро переключения службы Zapret (вкл/выкл) для UI и Telegram-бота.
/// В отличие от прямого вызова S51zapret синхронно обновляет cfg.zapret.enabled,
/// файлы zapret.conf, маркер автозапуска (Z-03) и стабилизирует процесс —
/// иначе watchdog откатывает действие бота через ≤12с, а GET /api/zapret/status убивает процесс.
pub async fn zapret_set_enabled_core(state: &AppState, enabled: bool) -> Result<String, String> {
    let init_script = "/opt/etc/init.d/S51zapret";

    if enabled {
        if !std::path::Path::new(init_script).exists() {
            return Err("Служба Zapret (S51zapret) не установлена в /opt/etc/init.d/".to_string());
        }
        {
            // Файлы службы пишутся с целевым флагом, но в config.json он попадает только после успешного старта
            let mut zcfg = state.config.read().await.zapret.clone();
            zcfg.enabled = true;
            sync_zapret_files_with_config_path(&zcfg, &state.config_path).await?;
        }
        crate::zapret::clear_zapret_disabled_marker();
        let out = tokio::process::Command::new(init_script)
            .arg("start")
            .output()
            .await
            .map_err(|e| {
                crate::zapret::set_zapret_disabled_marker();
                format!("Не удалось запустить S51zapret: {e}")
            })?;
        if !out.status.success() {
            // Баг 57: неудачный старт не оставляет enabled=true в конфиге
            crate::zapret::set_zapret_disabled_marker();
            return Err(format!(
                "Ошибка запуска Zapret:\n{}",
                format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)).trim()
            ));
        }
        if let Err(e) = persist_zapret_enabled_strict(state, true).await {
            // Баг 58: при ошибке reload Mihomo откатываем и службу, и конфиг
            crate::zapret::set_zapret_disabled_marker();
            let _ = tokio::process::Command::new(init_script).arg("stop").output().await;
            return Err(e);
        }
        let cur_cfg = state.config.read().await.clone();
        if cur_cfg.vkturn.enabled {
            let turn_ips = state.vkturn.rt.read().await.turn_ips.clone();
            crate::vkturn::apply_exemptions(&cur_cfg.vkturn.peer, &turn_ips).await;
        }
        Ok("🟢 Служба Zapret запущена".to_string())
    } else {
        // Баг 58: сначала транзакционно обновляем конфиг и reload Mihomo; при ошибке ничего не ломаем
        persist_zapret_enabled_strict(state, false).await?;
        crate::zapret::set_zapret_disabled_marker();
        let zcfg = state.config.read().await.zapret.clone();
        let _ = sync_zapret_files_with_config_path(&zcfg, &state.config_path).await;
        let _ = tokio::process::Command::new(init_script).arg("stop").output().await;
        Ok("⚪ Служба Zapret остановлена".to_string())
    }
}

/// Строгая транзакционная фиксация zapret.enabled с reload Mihomo без тихого fallback (баг 58).
pub async fn persist_zapret_enabled_strict(state: &AppState, enabled: bool) -> Result<(), String> {
    let mut tx = crate::transaction::ConfigTx::begin(state).await?;
    tx.config_mut().zapret.enabled = enabled;
    let raw = tx.read_yaml().await?;
    let (new_yaml, _) = routing::apply_routing(&raw, tx.config())?;
    tx.set_yaml(new_yaml)?;
    tx.commit_and_reload().await
}

/// Сохранение zapret.conf с пересборкой правил Mihomo (apply_routing) внутри ConfigTx (баг 30).
pub async fn apply_zapret_conf_save(
    state: &AppState,
    content: &str,
) -> Result<config::AppConfig, String> {
    let mut tx = crate::transaction::ConfigTx::begin(state).await?;
    tx.config_mut().zapret = crate::zapret::parse_zapret_conf(content, &tx.config().zapret);
    let new_cfg = tx.config().clone();

    // Если есть config.yaml Mihomo — пересобираем правила маршрутизации
    if let Ok(yaml) = tx.read_yaml().await {
        if let Ok((new_yaml, _)) = crate::routing::apply_routing(&yaml, &new_cfg) {
            tx.set_yaml(new_yaml)?;
        }
    }

    // Сохраняем и перезагружаем ядро Mihomo атомарно
    tx.commit_and_reload().await?;
    Ok(new_cfg)
}

pub async fn persist_zapret_enabled_with_routing(state: &AppState, enabled: bool) -> Result<(), String> {
    async fn attempt(state: &AppState, enabled: bool, reload: bool) -> Result<(), String> {
        let mut tx = crate::transaction::ConfigTx::begin(state).await?;
        tx.config_mut().zapret.enabled = enabled;
        let raw = tx.read_yaml().await?;
        let (new_yaml, _) = routing::apply_routing(&raw, tx.config())?;
        tx.set_yaml(new_yaml)?;
        if reload {
            tx.commit_and_reload().await
        } else {
            tx.commit_without_reload().await
        }
    }
    if let Err(e) = attempt(state, enabled, true).await {
        log_w!("[ZAPRET] Reload Mihomo не удался ({e}) — сохраняю конфигурацию без reload");
        attempt(state, enabled, false).await?;
    }
    Ok(())
}

/// Shell-скрипт параллельной проверки YouTube/Discord напрямую и через прокси Mihomo (кнопка «Проверка DPI»).
pub fn build_dpi_test_script(proxy_q: &str) -> String {
    format!(r#"
            t=$(mktemp -d 2>/dev/null || echo /tmp/xkr-dpi-$$); mkdir -p "$t"
            probe() {{ out=$(curl -4 -k -m 2.5 -s -o /dev/null -w "%{{http_code}}:%{{time_total}}" "$@" 2>/dev/null) || out="000:0.0"; [ -n "$out" ] || out="000:0.0"; echo "$out"; }}
            probe https://www.youtube.com/generate_204 > "$t/d_yt" &
            probe https://discord.com > "$t/d_dc" &
            probe -x '{proxy_q}' https://www.youtube.com/generate_204 > "$t/p_yt" &
            probe -x '{proxy_q}' https://discord.com > "$t/p_dc" &
            wait
            echo "$(cat "$t/d_yt")#$(cat "$t/d_dc")#$(cat "$t/p_yt")#$(cat "$t/p_dc")"
            rm -rf "$t"
        "#)
}

pub async fn zapret_action(
    State(state): State<AppState>,
    axum::extract::Json(body): axum::extract::Json<ZapretActionReq>,
) -> Response {
    let act = body.action.trim();

    // BUG-26: Межпроцессная блокировка и фиксация операционного состояния
    let op_state = match act {
        "start" => crate::zapret::ZapretOperationState::Starting,
        "stop" => crate::zapret::ZapretOperationState::Stopping,
        "restart" => crate::zapret::ZapretOperationState::Restarting,
        "upgrade_zapret2" | "rollback_v1" => crate::zapret::ZapretOperationState::Upgrading,
        "toggle_feature" | "set_features" | "reset_features" | "add_custom_domain" | "remove_custom_domain" | "toggle_custom_domain" | "boost_custom_domain" | "apply_strategy" => crate::zapret::ZapretOperationState::Reloading,
        _ => crate::zapret::ZapretOperationState::Configuring,
    };

    let _zapret_guard = if act != "test_dpi" {
        match crate::zapret::ZapretLockGuard::acquire(state.zapret_lock.clone(), op_state, 10).await {
            Ok(g) => Some(g),
            Err(e) => return api_err(format!("Служба Zapret занята другой операцией: {e}")),
        }
    } else {
        None
    };

    let proxy_url = state.config.read().await.mihomo_proxy_url();

    // 0.05 1-клик обновление до Zapret 2 (nfqws2 + Lua библиотеки в /opt/zapret2) с сохранением резервной копии v1
    if act == "upgrade_zapret2" {
        let hw = detect_zapret_hardware(&state).await;
        // Z-02: останавливаем службу ДО замены бинарника — иначе mv из /tmp (cross-device
        // copy+unlink) получает ETXTBSY от запущенного nfqws2, апгрейд проходит «фантомно»:
        // version.txt уже новой версии, а бинарник на диске — старый.
        let zap_enabled_before_upgrade = state.config.read().await.zapret.enabled;
        if zap_enabled_before_upgrade {
            let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret")
                .arg("stop")
                .output()
                .await;
        }
        let target_ver = {
            let requested = body.version.as_deref().unwrap_or("").trim();
            if !requested.is_empty() {
                if requested.starts_with('v') { requested.to_string() } else { format!("v{requested}") }
            } else {
                let cfg = state.config.read().await;
                let lat = cfg.zapret.latest_version.as_deref().unwrap_or("").trim();
                if !lat.is_empty() && lat != "v1.0.5.2" {
                    if lat.starts_with('v') { lat.to_string() } else { format!("v{lat}") }
                } else {
                    "v1.0.6".to_string()
                }
            }
        };
        let target_ver_no_v = target_ver.trim_start_matches('v').to_string();

        let upgrade_cmd = r#"
            PROXY="${MIHOMO_PROXY:-http://127.0.0.1:7890}"
            TARGET_VER="${TARGET_VER:-v1.0.6}"
            TARGET_VER_NO_V="${TARGET_VER_NO_V:-${TARGET_VER#v}}"
            ARCH=$(uname -m)
            case "$ARCH" in
              aarch64|arm64) TARGET_ARCH="linux-arm64"; Z2_ARCH="arm64" ;;
              armv7*|armv8*|arm*) TARGET_ARCH="linux-arm"; Z2_ARCH="arm" ;;
              mips*)
                if [ "$(hexdump -s 5 -n 1 -e '"%02x"' /bin/sh 2>/dev/null)" = "02" ] || \
                   [ "$(od -t x1 -j 5 -N 1 /bin/sh 2>/dev/null | awk 'NR==1{print $2}')" = "02" ] || \
                   [ "$(echo -n I | hexdump -o 2>/dev/null | awk '{ print substr($2,6,1); exit }')" = "0" ]; then
                  TARGET_ARCH="linux-mips32r2-msb"; Z2_ARCH="mips"
                else
                  TARGET_ARCH="linux-mips32r2-lsb"; Z2_ARCH="mipsel"
                fi
                ;;
              x86_64) TARGET_ARCH="linux-x86_64"; Z2_ARCH="x86_64" ;;
              *) TARGET_ARCH="linux-arm64"; Z2_ARCH="arm64" ;;
            esac

            echo "[1/4] Архитектура целевой системы: $TARGET_ARCH ($ARCH, Z2_ARCH=$Z2_ARCH)"

            mkdir -p /opt/zapret2 /opt/zapret2/lua /opt/zapret2/binaries /opt/etc/init.d /opt/etc/zapret /opt/sbin /opt/zapret /opt/zapret/nfq

            # Сохраняем резервную копию конфигурации и бинарника v1 для безопасного переключения
            if [ -f /opt/etc/zapret/zapret.conf ] && ! grep -q "lua-desync" /opt/etc/zapret/zapret.conf 2>/dev/null; then
              cp -f /opt/etc/zapret/zapret.conf /opt/etc/zapret/zapret.v1.conf.bak 2>/dev/null || true
              echo "Создана резервная копия конфигурации: /opt/etc/zapret/zapret.v1.conf.bak"
            fi
            for v1bin in /opt/zapret/nfq/nfqws /opt/sbin/nfqws /opt/bin/nfqws /opt/usr/bin/nfqws; do
              if [ -x "$v1bin" ] && [ ! -f /opt/zapret/nfqws.bak ]; then
                cp -f "$v1bin" /opt/zapret/nfqws.bak 2>/dev/null || true
                chmod +x /opt/zapret/nfqws.bak 2>/dev/null || true
                echo "Создана резервная копия исполняемого файла v1: /opt/zapret/nfqws.bak"
                break
              fi
            done

            STAGE_DIR=$(mktemp -d /tmp/z2_stage.XXXXXX)
            echo "[2/4] Загрузка дистрибутива Zapret 2 ($TARGET_VER) в $STAGE_DIR..."
            for url in \
              "https://github.com/bol-van/zapret2/releases/download/$TARGET_VER/zapret2-$TARGET_VER.tar.gz" \
              "https://github.com/bol-van/zapret2/releases/latest/download/zapret2-$TARGET_VER.tar.gz" \
              "https://github.com/bol-van/zapret2/releases/download/$TARGET_VER/zapret2-$TARGET_VER_NO_V.tar.gz" \
              "https://github.com/bol-van/zapret2/archive/refs/tags/$TARGET_VER.tar.gz"; do
              if curl -fsSL --connect-timeout 10 -m 60 -x "$PROXY" "$url" -o "$STAGE_DIR/z2.tar.gz" 2>/dev/null || \
                 curl -fsSL --connect-timeout 10 -m 60 "$url" -o "$STAGE_DIR/z2.tar.gz" 2>/dev/null; then
                if [ -s "$STAGE_DIR/z2.tar.gz" ] && [ $(wc -c < "$STAGE_DIR/z2.tar.gz" 2>/dev/null || echo 0) -gt 10000 ]; then
                  echo "Архив успешно загружен с: $url"
                  break
                fi
              fi
              rm -f "$STAGE_DIR/z2.tar.gz"
            done

            echo "[3/4] Распаковка архива и копирование бинарных файлов nfqws2 и Lua модулей..."
            if [ -f "$STAGE_DIR/z2.tar.gz" ]; then
              tar -xzf "$STAGE_DIR/z2.tar.gz" -C "$STAGE_DIR" 2>/dev/null || true
              rm -f "$STAGE_DIR/z2.tar.gz"
              Z2_DIR=$(find "$STAGE_DIR" -maxdepth 1 -type d \( -name "zapret2*" -o -name "zapret-v*" \) | head -n 1)
              if [ -n "$Z2_DIR" ]; then
                echo "Распакован каталог сборки: $Z2_DIR"
                if [ -d "$Z2_DIR/binaries" ]; then
                  cp -rf "$Z2_DIR/binaries/"* /opt/zapret2/binaries/ 2>/dev/null || true
                fi
                NEW_BIN=""
                if [ -n "$Z2_ARCH" ] && [ -f "$Z2_DIR/binaries/$Z2_ARCH/nfqws2" ]; then
                  NEW_BIN="$Z2_DIR/binaries/$Z2_ARCH/nfqws2"
                elif [ -f "$Z2_DIR/binaries/$TARGET_ARCH/nfqws2" ]; then
                  NEW_BIN="$Z2_DIR/binaries/$TARGET_ARCH/nfqws2"
                elif [ -f "$Z2_DIR/nfqws2" ]; then
                  NEW_BIN="$Z2_DIR/nfqws2"
                fi
                if [ -n "$NEW_BIN" ] && [ -f "$NEW_BIN" ]; then
                  MAGIC=$(hexdump -n 4 -e '4/1 "%02x"' "$NEW_BIN" 2>/dev/null || od -An -tx1 -N4 "$NEW_BIN" 2>/dev/null | tr -d ' \r\n')
                  if [ "$MAGIC" != "7f454c46" ]; then
                    echo "ERROR: Invalid ELF header in $NEW_BIN (expected 7f 45 4c 46, got $MAGIC)" >&2
                    rm -rf "$STAGE_DIR"
                    exit 1
                  fi
                  # Z-02: верификация реальной замены бинарника (защита от молчаливого ETXTBSY)
                  EXPECTED_SIZE=$(wc -c < "$NEW_BIN" 2>/dev/null || echo 0)
                  kill $(pidof nfqws2 2>/dev/null) 2>/dev/null || true
                  sleep 1
                  if mv -f "$NEW_BIN" /opt/zapret2/nfqws2 2>/dev/null; then
                    chmod +x /opt/zapret2/nfqws2 2>/dev/null || true
                    INSTALLED_SIZE=$(wc -c < /opt/zapret2/nfqws2 2>/dev/null || echo 0)
                    if [ "$EXPECTED_SIZE" != "$INSTALLED_SIZE" ]; then
                      echo "ERROR: nfqws2 size mismatch after install (expected $EXPECTED_SIZE, got $INSTALLED_SIZE)" >&2
                      rm -rf "$STAGE_DIR"
                      exit 1
                    fi
                    echo "Скопирован nfqws2"
                  else
                    echo "ERROR: не удалось заменить /opt/zapret2/nfqws2 (binary busy?)" >&2
                    rm -rf "$STAGE_DIR"
                    exit 1
                  fi
                fi
                if [ -d "$Z2_DIR/files/lua" ]; then
                  cp -rf "$Z2_DIR/files/lua/"* /opt/zapret2/lua/ 2>/dev/null || true
                  echo "Скопированы Lua библиотеки из files/lua в /opt/zapret2/lua"
                fi
                if [ -d "$Z2_DIR/lua" ]; then
                  cp -rf "$Z2_DIR/lua/"* /opt/zapret2/lua/ 2>/dev/null || true
                  echo "Скопированы Lua библиотеки из lua в /opt/zapret2/lua"
                fi
              fi
            fi
            rm -rf "$STAGE_DIR" 2>/dev/null || true

            # Если ключевые Lua скрипты отсутствуют или повреждены, загружаем их напрямую из официального репозитория
            for lf in zapret-lib.lua zapret-antidpi.lua zapret-auto.lua; do
              if [ ! -s "/opt/zapret2/lua/$lf" ] || [ $(wc -c < "/opt/zapret2/lua/$lf" 2>/dev/null || echo 0) -lt 80 ] || grep -q "404: Not Found" "/opt/zapret2/lua/$lf" 2>/dev/null; then
                rm -f "/opt/zapret2/lua/$lf"
                (curl -fsSL -x "$PROXY" "https://raw.githubusercontent.com/bol-van/zapret2/master/lua/$lf" -o "/opt/zapret2/lua/$lf" || \
                 curl -fsSL "https://raw.githubusercontent.com/bol-van/zapret2/master/lua/$lf" -o "/opt/zapret2/lua/$lf") 2>/dev/null || true
                if [ $(wc -c < "/opt/zapret2/lua/$lf" 2>/dev/null || echo 0) -lt 80 ] || grep -q "404: Not Found" "/opt/zapret2/lua/$lf" 2>/dev/null; then
                  rm -f "/opt/zapret2/lua/$lf"
                fi
              fi
            done

            echo "[4/4] Настройка прав доступа, симлинков и инициализация /opt/zapret2..."
            [ -f /opt/zapret2/nfqws2 ] && chmod +x /opt/zapret2/nfqws2 && ln -sf /opt/zapret2/nfqws2 /opt/sbin/nfqws2
            chmod 644 /opt/zapret2/lua/*.lua 2>/dev/null || true
            echo "$TARGET_VER" > /opt/zapret2/version.txt
            echo "Установка Zapret 2.0 ($TARGET_VER) успешно завершена."
        "#;

        match tokio::process::Command::new("sh")
            .arg("-c")
            .arg(upgrade_cmd)
            .env("MIHOMO_PROXY", &proxy_url)
            .env("TARGET_VER", &target_ver)
            .env("TARGET_VER_NO_V", &target_ver_no_v)
            .output()
            .await
        {
            Ok(out) => {
                let output_str = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
                if !is_nfqws2_available() {
                    return api_err(format!(
                        "Не удалось установить Zapret 2.0: исполняемый файл nfqws2 или Lua-библиотеки не найдены в /opt/zapret2. Проверьте подключение к сети. Текущая конфигурация v1 сохранена без изменений.\nЛог:\n{}",
                        output_str.trim()
                    ));
                }

                let _ = tokio::fs::create_dir_all("/opt/zapret2").await;
                let _ = tokio::fs::write("/opt/zapret2/version.txt", format!("{target_ver}\n")).await;

                let _cfg_guard = state.config_lock.lock().await;
                let mut cfg = (**state.config.read().await).clone();
                cfg.zapret.engine = "v2".to_string();
                cfg.zapret.update_available = false;
                cfg.zapret.latest_version = Some(target_ver.clone());
                cfg.zapret.last_update_check = Some(chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string());
                if let Some(ref ca) = cfg.zapret.custom_args {
                    if ca.contains("--dpi-desync") && !ca.contains("--lua-desync") {
                        cfg.zapret.custom_args = None;
                    }
                }
                if let Err(e) = sync_zapret_files(&cfg.zapret).await {
                    return api_err(format!("Ошибка синхронизации файлов Zapret 2.0: {e}"));
                }
                let _ = config::save(&state.config_path, &cfg).await;
                *state.config.write().await = std::sync::Arc::new(cfg.clone());

                if cfg.zapret.enabled {
                    let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("restart").output().await;
                }

                return api_ok(json!({
                    "success": true,
                    "engine": "v2",
                    "v2_installed": true,
                    "v1_installed": is_nfqws1_available(),
                    "can_rollback_v1": true,
                    "hardware": hw,
                    "features": cfg.zapret,
                    "version": &target_ver,
                    "latest_version": &target_ver,
                    "update_available": false,
                    "output": output_str.trim(),
                    "message": format!("Zapret 2.0 ({target_ver}, nfqws2 + Lua) установлен в /opt/zapret2 для {} ({})", hw.model, hw.arch_label)
                }));
            }
            Err(e) => return api_err(format!("Ошибка обновления до Zapret 2.0: {e}")),
        }
    }

    // 0.06 Мгновенное переключение движка (Запрет 1 / Запрет 2)
    if act == "rollback_v1" || act == "switch_engine" {
        let target_engine = if act == "rollback_v1" {
            "v1"
        } else {
            normalize_engine_choice(body.engine.as_deref().unwrap_or("v2"))
        };

        if target_engine == "v1" {
            let restore_v1_cmd = r#"
                mkdir -p /opt/zapret/nfq /opt/etc/zapret
                if [ ! -x /opt/zapret/nfq/nfqws ]; then
                  if [ -x /opt/zapret/nfqws.bak ]; then
                    cp -f /opt/zapret/nfqws.bak /opt/zapret/nfq/nfqws 2>/dev/null || true
                  elif [ -x /opt/sbin/nfqws ]; then
                    cp -f /opt/sbin/nfqws /opt/zapret/nfq/nfqws 2>/dev/null || true
                  elif [ -x /opt/bin/nfqws ]; then
                    cp -f /opt/bin/nfqws /opt/zapret/nfq/nfqws 2>/dev/null || true
                  fi
                  chmod +x /opt/zapret/nfq/nfqws 2>/dev/null || true
                fi
                if [ ! -x /opt/zapret/nfq/nfqws ] && [ ! -x /opt/sbin/nfqws ]; then
                  ARCH=$(uname -m)
                  case "$ARCH" in
                    aarch64|arm64) TARGET_ARCH="linux-arm64" ;;
                    armv7*|armv8*|arm*) TARGET_ARCH="linux-arm" ;;
                    mips*)
                      if [ "$(hexdump -s 5 -n 1 -e '"%02x"' /bin/sh 2>/dev/null)" = "02" ]; then
                        TARGET_ARCH="linux-mips32r2-msb"
                      else
                        TARGET_ARCH="linux-mips32r2-lsb"
                      fi
                      ;;
                    x86_64) TARGET_ARCH="linux-x86_64" ;;
                    *) TARGET_ARCH="linux-arm64" ;;
                  esac
                  PROXY="${MIHOMO_PROXY:-http://127.0.0.1:7890}"
                  for url in \
                    "https://github.com/bol-van/zapret/releases/download/v72.13/zapret-v72.13.tar.gz"; do
                    if curl -fsSL --connect-timeout 10 -m 60 -x "$PROXY" "$url" -o /tmp/z1.tar.gz 2>/dev/null || \
                       curl -fsSL --connect-timeout 10 -m 60 "$url" -o /tmp/z1.tar.gz 2>/dev/null; then
                      tar -xzf /tmp/z1.tar.gz -C /tmp/ 2>/dev/null || true
                      rm -f /tmp/z1.tar.gz
                      Z1_DIR=$(find /tmp -maxdepth 1 -type d -name "zapret-v*" | head -n 1)
                      if [ -n "$Z1_DIR" ] && [ -f "$Z1_DIR/binaries/$TARGET_ARCH/nfqws" ]; then
                        cp -f "$Z1_DIR/binaries/$TARGET_ARCH/nfqws" /opt/zapret/nfq/nfqws
                        chmod +x /opt/zapret/nfq/nfqws
                        ln -sf /opt/zapret/nfq/nfqws /opt/sbin/nfqws
                      fi
                      rm -rf "$Z1_DIR" 2>/dev/null || true
                      break
                    fi
                  done
                fi
            "#;
            let _ = tokio::process::Command::new("sh").arg("-c").arg(restore_v1_cmd).env("MIHOMO_PROXY", &proxy_url).output().await;

            if !is_nfqws1_available() && !std::path::Path::new("/opt/etc/zapret/zapret.v1.conf.bak").exists() {
                return api_err("Не удалось активировать Zapret 1: исполняемый файл nfqws не найден.");
            }
        } else if target_engine == "v2" && !is_nfqws2_available() {
            // Автоматическая установка Zapret 2 при переключении тумблера, если он ещё не установлен
            return Box::pin(zapret_action(
                State(state),
                axum::extract::Json(ZapretActionReq {
                    action: "upgrade_zapret2".to_string(),
                    preset: None,
                    custom_args: None,
                    config_content: None,
                    hosts_content: None,
                    feature: None,
                    enabled: None,
                    features: None,
                    domain: None,
                    strategy_id: None,
                    url: None,
                    engine: Some("v2".to_string()),
                    version: None,
                }),
            ))
            .await;
        }

        let _cfg_guard = state.config_lock.lock().await;
        let mut cfg = (**state.config.read().await).clone();
        cfg.zapret.engine = target_engine.to_string();
        if target_engine == "v1" {
            cfg.zapret.update_available = false;
        }

        if target_engine == "v1" {
            if let Some(ref ca) = cfg.zapret.custom_args {
                if ca.contains("--lua-desync") || ca.contains("multisplit") {
                    cfg.zapret.custom_args = Some(convert_lua_to_legacy_desync(ca));
                }
            }
        } else if let Some(ref ca) = cfg.zapret.custom_args {
            if ca.contains("--dpi-desync") && !ca.contains("--lua-desync") {
                cfg.zapret.custom_args = None;
            }
        }

        if let Err(e) = sync_zapret_files(&cfg.zapret).await {
            return api_err(format!("Ошибка синхронизации файлов Zapret при смене движка: {e}"));
        }
        let _ = config::save(&state.config_path, &cfg).await;
        *state.config.write().await = std::sync::Arc::new(cfg.clone());

        if cfg.zapret.enabled {
            let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("restart").output().await;
        }

        let msg = if target_engine == "v1" {
            "Переключено на движок Zapret 1 (nfqws)"
        } else {
            "Переключено на движок Zapret 2 (nfqws2 + Lua)"
        };

        return api_ok(json!({
            "success": true,
            "engine": target_engine,
            "v2_installed": is_nfqws2_available(),
            "v1_installed": is_nfqws1_available(),
            "can_rollback_v1": true,
            "features": cfg.zapret,
            "message": msg
        }));
    }

    // 0.1 Применение выбранной стратегии автоподбора
    if act == "apply_strategy" {
        let _cfg_guard = state.config_lock.lock().await;
        let mut cfg = (**state.config.read().await).clone();
        let prev_zapret = cfg.zapret.clone();
        let custom_args = body.custom_args.as_ref().filter(|a| !a.trim().is_empty()).map(|a| a.trim().to_string());
        let final_args = match custom_args {
            Some(a) => Some(a),
            None => body.strategy_id.as_deref().and_then(get_strategy_args_by_id).map(|s| s.to_string()),
        };
        if let Some(ref args) = final_args {
            if let Err(e) = validate_custom_args(args) {
                return api_err(e);
            }
            let effective_args = if !should_use_nfqws2(&cfg.zapret) && (args.contains("--lua-desync") || args.contains("multisplit")) {
                convert_lua_to_legacy_desync(args)
            } else {
                args.clone()
            };
            cfg.zapret.custom_args = Some(effective_args);
        }
        if let Err(e) = sync_zapret_files(&cfg.zapret).await {
            return api_err(format!("Ошибка синхронизации файлов Zapret: {e}"));
        }
        let _ = config::save(&state.config_path, &cfg).await;
        *state.config.write().await = std::sync::Arc::new(cfg.clone());
        if cfg.zapret.enabled {
            match tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("restart").output().await {
                Ok(out) if !out.status.success() => {
                    let err_msg = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
                    cfg.zapret = prev_zapret;
                    let _ = sync_zapret_files(&cfg.zapret).await;
                    let _ = config::save(&state.config_path, &cfg).await;
                    *state.config.write().await = std::sync::Arc::new(cfg);
                    let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("restart").output().await;
                    return api_err(format!("Ошибка применения стратегии Zapret:\n{}", err_msg.trim()));
                }
                Err(e) => {
                    // Z-10: rollback симметричен ветке Ok(!success) — конфиг уже был
                    // сохранён с новой стратегией, откатываем файлы и состояние
                    cfg.zapret = prev_zapret;
                    let _ = sync_zapret_files(&cfg.zapret).await;
                    let _ = config::save(&state.config_path, &cfg).await;
                    *state.config.write().await = std::sync::Arc::new(cfg);
                    let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("restart").output().await;
                    return api_err(format!("Ошибка перезапуска S51zapret: {e} (конфигурация откачена)"));
                }
                _ => {}
            }
        }
        return api_ok(json!({
            "success": true,
            "active_strategy_id": body.strategy_id,
            "message": "Стратегия десинхронизации успешно применена",
            "features": cfg.zapret
        }));
    }

    // 0.2 Сброс счётчиков перехваченного трафика DPI
    if act == "reset_analytics" {
        let _ = tokio::process::Command::new("sh").arg("-c").arg("iptables -w 2 -t mangle -Z zapret 2>/dev/null || true").output().await;
        ZAPRET_LAST_IPT_BYTES.store(0, std::sync::atomic::Ordering::Relaxed);
        let _cfg_guard = state.config_lock.lock().await;
        let mut cfg = (**state.config.read().await).clone();
        cfg.zapret.period_saved_bytes = 0;
        let _ = config::save(&state.config_path, &cfg).await;
        *state.config.write().await = std::sync::Arc::new(cfg);
        return api_ok(json!({ "success": true, "message": "Счётчики перехваченного трафика DPI сброшены" }));
    }

    // 0.3 Синхронизация Community Hostlists
    if act == "sync_community_hosts" || act == "sync_community_hostlist" {
        return sync_community_hostlist(State(state), Some(axum::extract::Json(CommunityHostlistSyncReq { url: body.url.clone() }))).await;
    }

    // 1. Тестирование обхода DPI (прямой через Zapret и через Mihomo прокси)
    if act == "test_dpi" {
        let is_running = crate::zapret::find_our_zapret_pid().await.is_some();

        let ipt_ok = tokio::process::Command::new("sh")
            .arg("-c")
            .arg("(iptables -w 2 -t mangle -S PREROUTING 2>/dev/null | grep -q zapret || iptables -w 2 -t mangle -S POSTROUTING 2>/dev/null | grep -q zapret) && iptables -w 2 -t mangle -nL zapret 2>/dev/null | grep -q NFQUEUE")
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false);

        let cfg = state.config.read().await.clone();
        let proxy_url = cfg.mihomo_proxy_url();
        // Z-13: URL попадает в sh -c без экранирования — вырезаем кавычки/бэкслеши
        // (ломали команду, метасимволы давали инъекцию) и оборачиваем в одинарные кавычки
        let proxy_q = proxy_url.replace(['\'', '\\', '`', '$'], "");

        // Быстрая параллельная проверка доступности: прямой запрос и через прокси Mihomo (макс 2.5 сек)
        let test_cmd = build_dpi_test_script(&proxy_q);
        let out = tokio::process::Command::new("sh").arg("-c").arg(&test_cmd).output().await;
        let line = out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
        let parts: Vec<&str> = line.split('#').collect();

        let parse_one = |s: &str| -> (u16, f64) {
            let mut split = s.split(':');
            let code = split.next().and_then(|c| c.parse().ok()).unwrap_or(0);
            let time = split.next().and_then(|t| t.parse().ok()).unwrap_or(0.0);
            (code, time)
        };

        let (yt_d_code, yt_d_time) = parts.first().map(|s| parse_one(s)).unwrap_or((0, 0.0));
        let (dc_d_code, dc_d_time) = parts.get(1).map(|s| parse_one(s)).unwrap_or((0, 0.0));
        let (yt_p_code, yt_p_time) = parts.get(2).map(|s| parse_one(s)).unwrap_or((0, 0.0));
        let (dc_p_code, dc_p_time) = parts.get(3).map(|s| parse_one(s)).unwrap_or((0, 0.0));

        let yt_d_ok = (200..400).contains(&yt_d_code);
        let dc_d_ok = (200..400).contains(&dc_d_code);

        return api_ok(json!({
            "youtube": {
                "code": yt_d_code,
                "time_secs": yt_d_time,
                "ok": yt_d_ok,
                "proxy_code": yt_p_code,
                "proxy_time_secs": yt_p_time,
                "proxy_ok": (200..400).contains(&yt_p_code),
            },
            "discord": {
                "code": dc_d_code,
                "time_secs": dc_d_time,
                "ok": dc_d_ok,
                "proxy_code": dc_p_code,
                "proxy_time_secs": dc_p_time,
                "proxy_ok": (200..400).contains(&dc_p_code),
            },
            "service_running": is_running,
            "iptables_active": ipt_ok,
        }));
    }

    // 2. Сохранение конфигурации zapret.conf
    if act == "save_config" {
        if let Some(content) = body.config_content {
            if let Err(e) = validate_zapret_conf(&content) {
                return api_err(format!("Ошибка валидации zapret.conf: {e}"));
            }
            let prev_conf = tokio::fs::read_to_string("/opt/etc/zapret/zapret.conf").await.ok();

            let _ = tokio::fs::create_dir_all("/opt/etc/zapret").await;
            if let Err(e) = atomic_write_file("/opt/etc/zapret/zapret.conf", &content).await {
                return api_err(format!("Ошибка записи zapret.conf: {e}"));
            }

            // Баг 30: пересборка правил Mihomo (apply_routing) и сохранение в ConfigTx
            let cfg = match apply_zapret_conf_save(&state, &content).await {
                Ok(c) => c,
                Err(e) => {
                    if let Some(prev) = prev_conf {
                        let _ = tokio::fs::write("/opt/etc/zapret/zapret.conf", prev).await;
                    }
                    return api_err(format!("Ошибка применения конфигурации Zapret: {e}"));
                }
            };

            if cfg.zapret.enabled {
                match tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("restart").output().await {
                    Ok(out) if !out.status.success() => {
                        let err_msg = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
                        if let Some(ref prev) = prev_conf {
                            let _ = tokio::fs::write("/opt/etc/zapret/zapret.conf", prev).await;
                            let _ = apply_zapret_conf_save(&state, prev).await;
                        } else {
                            let _ = sync_zapret_files(&cfg.zapret).await;
                        }
                        let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("restart").output().await;
                        return api_err(format!("Ошибка перезапуска Zapret с новой конфигурацией:\n{}", err_msg.trim()));
                    }
                    Err(e) => {
                        if let Some(ref prev) = prev_conf {
                            let _ = tokio::fs::write("/opt/etc/zapret/zapret.conf", prev).await;
                            let _ = apply_zapret_conf_save(&state, prev).await;
                        }
                        return api_err(format!("Ошибка вызова S51zapret restart: {e}"));
                    }
                    _ => {}
                }
            }

            let msg = if cfg.zapret.enabled {
                "Конфигурация Zapret сохранена и служба перезапущена"
            } else {
                "Конфигурация Zapret сохранена"
            };
            return api_ok(json!({
                "saved": true,
                "hot_reloaded": false,
                "restart_required": !cfg.zapret.enabled,
                "features": cfg.zapret,
                "message": msg
            }));
        }
        return api_err("Отсутствует содержимое config_content");
    }

    // 2.1 Сохранение списка доменов zapret-hosts.txt
    if act == "save_hosts" {
        if let Some(content) = body.hosts_content {
            let _ = tokio::fs::create_dir_all("/opt/etc/zapret").await;
            if let Err(e) = atomic_write_file("/opt/etc/zapret/zapret-hosts.txt", &content).await {
                return api_err(format!("Ошибка записи zapret-hosts.txt: {e}"));
            }
            let is_running = crate::zapret::find_our_zapret_pid().await.is_some();
            if is_running {
                let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("reload-hosts").output().await;
            }
            return api_ok(json!({
                "saved": true,
                "hot_reloaded": is_running,
                "restart_required": !is_running,
                "message": "Список доменов zapret-hosts.txt сохранен"
            }));
        }
        return api_err("Отсутствует содержимое hosts_content");
    }

    // 3. Выбор пресета / быстрых бандлов стратегий
    if act == "set_preset" {
        let p = body.preset.as_deref().unwrap_or("general");
        match apply_zapret_preset_action(&state, p, body.custom_args.as_deref()).await {
            Ok(msg) => {
                let cfg = state.config.read().await;
                return api_ok(json!({ "preset": p, "features": cfg.zapret, "message": msg }));
            }
            Err(e) => return api_err(e),
        }
    }

    // 4. Установка службы Zapret
    if act == "install" {
        // Z-02: останавливаем запущенную службу до копирования бинарника (ETXTBSY)
        let zap_enabled_before_install = state.config.read().await.zapret.enabled;
        if zap_enabled_before_install {
            let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret")
                .arg("stop")
                .output()
                .await;
        }
        let target_ver = {
            let requested = body.version.as_deref().unwrap_or("").trim();
            if !requested.is_empty() {
                if requested.starts_with('v') { requested.to_string() } else { format!("v{requested}") }
            } else {
                let cfg = state.config.read().await;
                let lat = cfg.zapret.latest_version.as_deref().unwrap_or("").trim();
                if !lat.is_empty() && lat != "v1.0.5.2" {
                    if lat.starts_with('v') { lat.to_string() } else { format!("v{lat}") }
                } else {
                    "v1.0.6".to_string()
                }
            }
        };
        let target_ver_no_v = target_ver.trim_start_matches('v').to_string();

        let install_cmd = r#"
            TARGET_VER="${TARGET_VER:-v1.0.6}"
            TARGET_VER_NO_V="${TARGET_VER_NO_V:-${TARGET_VER#v}}"
            ARCH=$(uname -m)
            case "$ARCH" in
              aarch64|arm64) TARGET_ARCH="linux-arm64"; Z2_ARCH="arm64" ;;
              armv7*|armv8*|arm*) TARGET_ARCH="linux-arm"; Z2_ARCH="arm" ;;
              mips*)
                if [ "$(hexdump -s 5 -n 1 -e '"%02x"' /bin/sh 2>/dev/null)" = "02" ] || \
                   [ "$(od -t x1 -j 5 -N 1 /bin/sh 2>/dev/null | awk 'NR==1{print $2}')" = "02" ] || \
                   [ "$(echo -n I | hexdump -o 2>/dev/null | awk '{ print substr($2,6,1); exit }')" = "0" ]; then
                  TARGET_ARCH="linux-mips32r2-msb"; Z2_ARCH="mips"
                else
                  TARGET_ARCH="linux-mips32r2-lsb"; Z2_ARCH="mipsel"
                fi
                ;;
              x86_64) TARGET_ARCH="linux-x86_64"; Z2_ARCH="x86_64" ;;
              *) TARGET_ARCH="linux-arm64"; Z2_ARCH="arm64" ;;
            esac

            echo "[1/4] Архитектура целевой системы: $TARGET_ARCH ($ARCH, Z2_ARCH=$Z2_ARCH)"

            mkdir -p /opt/zapret2 /opt/zapret2/lua /opt/zapret2/binaries /opt/etc/init.d /opt/etc/zapret /opt/sbin /opt/zapret

            cd /opt
            echo "[2/4] Загрузка дистрибутива Zapret 2 ($TARGET_VER)..."
            rm -f /tmp/z2.tar.gz
            PROXY="${MIHOMO_PROXY:-http://127.0.0.1:7890}"
            for url in \
              "https://github.com/bol-van/zapret2/releases/download/$TARGET_VER/zapret2-$TARGET_VER.tar.gz" \
              "https://github.com/bol-van/zapret2/releases/latest/download/zapret2-$TARGET_VER.tar.gz" \
              "https://github.com/bol-van/zapret2/releases/download/$TARGET_VER/zapret2-$TARGET_VER_NO_V.tar.gz" \
              "https://github.com/bol-van/zapret2/archive/refs/tags/$TARGET_VER.tar.gz"; do
              if curl -fsSL --connect-timeout 10 -m 60 -x "$PROXY" "$url" -o /tmp/z2.tar.gz 2>/dev/null || \
                 curl -fsSL --connect-timeout 10 -m 60 "$url" -o /tmp/z2.tar.gz 2>/dev/null; then
                if [ -s /tmp/z2.tar.gz ] && [ $(wc -c < /tmp/z2.tar.gz 2>/dev/null || echo 0) -gt 10000 ]; then
                  break
                fi
              fi
              rm -f /tmp/z2.tar.gz
            done

            echo "[3/4] Распаковка архива и копирование бинарных файлов nfqws2 и Lua модулей..."
            if [ -f /tmp/z2.tar.gz ]; then
              tar -xzf /tmp/z2.tar.gz -C /tmp/ 2>/dev/null || true
              rm -f /tmp/z2.tar.gz
              Z2_DIR=$(find /tmp -maxdepth 1 -type d \( -name "zapret2*" -o -name "zapret-v*" \) | head -n 1)
              if [ -n "$Z2_DIR" ]; then
                echo "Распакован каталог сборки: $Z2_DIR"
                if [ -d "$Z2_DIR/binaries" ]; then
                  cp -rf "$Z2_DIR/binaries/"* /opt/zapret2/binaries/ 2>/dev/null || true
                fi
                if [ -n "$Z2_ARCH" ] && [ -f "$Z2_DIR/binaries/$Z2_ARCH/nfqws2" ]; then
                  kill $(pidof nfqws2 2>/dev/null) 2>/dev/null || true; sleep 1
                  cp -f "$Z2_DIR/binaries/$Z2_ARCH/nfqws2" /opt/zapret2/nfqws2 || { echo "ERROR: не удалось заменить /opt/zapret2/nfqws2 (busy?)" >&2; exit 1; }
                  echo "Скопирован nfqws2 для $Z2_ARCH"
                elif [ -f "$Z2_DIR/binaries/$TARGET_ARCH/nfqws2" ]; then
                  kill $(pidof nfqws2 2>/dev/null) 2>/dev/null || true; sleep 1
                  cp -f "$Z2_DIR/binaries/$TARGET_ARCH/nfqws2" /opt/zapret2/nfqws2 || { echo "ERROR: не удалось заменить /opt/zapret2/nfqws2 (busy?)" >&2; exit 1; }
                  echo "Скопирован nfqws2 для $TARGET_ARCH"
                elif [ -f "$Z2_DIR/nfqws2" ]; then
                  kill $(pidof nfqws2 2>/dev/null) 2>/dev/null || true; sleep 1
                  cp -f "$Z2_DIR/nfqws2" /opt/zapret2/nfqws2 || { echo "ERROR: не удалось заменить /opt/zapret2/nfqws2 (busy?)" >&2; exit 1; }
                  echo "Скопирован nfqws2"
                else
                  echo "ОШИБКА: nfqws2 не найден в архиве Zapret 2 для $TARGET_ARCH" >&2
                  rm -rf "$Z2_DIR" /tmp/zapret-v* 2>/dev/null || true
                  exit 1
                fi
                if [ -d "$Z2_DIR/files/lua" ]; then
                  cp -rf "$Z2_DIR/files/lua/"* /opt/zapret2/lua/ 2>/dev/null || true
                  echo "Скопированы Lua библиотеки из files/lua в /opt/zapret2/lua"
                fi
                if [ -d "$Z2_DIR/lua" ]; then
                  cp -rf "$Z2_DIR/lua/"* /opt/zapret2/lua/ 2>/dev/null || true
                  echo "Скопированы Lua библиотеки из lua в /opt/zapret2/lua"
                fi
                rm -rf "$Z2_DIR" /tmp/zapret-v* 2>/dev/null || true
              fi
            fi

            for lf in zapret-lib.lua zapret-antidpi.lua zapret-auto.lua; do
              if [ ! -s "/opt/zapret2/lua/$lf" ] || [ $(wc -c < "/opt/zapret2/lua/$lf" 2>/dev/null || echo 0) -lt 80 ] || grep -q "404: Not Found" "/opt/zapret2/lua/$lf" 2>/dev/null; then
                rm -f "/opt/zapret2/lua/$lf"
                (curl -sSL -x "$PROXY" "https://raw.githubusercontent.com/bol-van/zapret2/master/lua/$lf" -o "/opt/zapret2/lua/$lf" || \
                 curl -sSL "https://raw.githubusercontent.com/bol-van/zapret2/master/lua/$lf" -o "/opt/zapret2/lua/$lf") 2>/dev/null || true
                if [ $(wc -c < "/opt/zapret2/lua/$lf" 2>/dev/null || echo 0) -lt 80 ] || grep -q "404: Not Found" "/opt/zapret2/lua/$lf" 2>/dev/null; then
                  rm -f "/opt/zapret2/lua/$lf"
                fi
              fi
            done

            echo "[4/4] Настройка прав доступа, симлинков и инициализация /opt/zapret2..."
            [ -f /opt/zapret2/nfqws2 ] && chmod +x /opt/zapret2/nfqws2 && ln -sf /opt/zapret2/nfqws2 /opt/sbin/nfqws2
            chmod 644 /opt/zapret2/lua/*.lua 2>/dev/null || true
            echo "$TARGET_VER" > /opt/zapret2/version.txt
            echo "Установка Zapret 2.0 ($TARGET_VER) успешно завершена."
        "#;
        match tokio::process::Command::new("sh")
            .arg("-c")
            .arg(install_cmd)
            .env("MIHOMO_PROXY", &proxy_url)
            .env("TARGET_VER", &target_ver)
            .env("TARGET_VER_NO_V", &target_ver_no_v)
            .output()
            .await
        {
            Ok(out) => {
                let output_str = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
                let _ = tokio::fs::create_dir_all("/opt/zapret2").await;
                let _ = tokio::fs::write("/opt/zapret2/version.txt", format!("{target_ver}\n")).await;
                {
                    let _cfg_guard = state.config_lock.lock().await;
                    let mut cfg = (**state.config.read().await).clone();
                    cfg.zapret.latest_version = Some(target_ver.clone());
                    cfg.zapret.update_available = false;
                    let _ = config::save(&state.config_path, &cfg).await;
                    *state.config.write().await = std::sync::Arc::new(cfg);
                }
                let _cfg = state.config.read().await;
                let _ = sync_zapret_files(&_cfg.zapret).await;
                if let Ok(out) = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("start").output().await {
                    if !out.status.success() {
                        return api_err(format!("Ошибка запуска Zapret: {}", String::from_utf8_lossy(&out.stderr)));
                    }
                }
                let installed = std::path::Path::new("/opt/etc/init.d/S51zapret").exists();
                return api_ok(json!({
                    "success": installed,
                    "version": &target_ver,
                    "latest_version": &target_ver,
                    "update_available": false,
                    "output": output_str.trim()
                }));
            }
            Err(e) => return api_err(format!("Ошибка установки Zapret: {}", e)),
        }
    }

    // 5. Управление независимыми режимами и выключателями (toggle_feature / set_features / reset_features)
    if act == "toggle_feature" || act == "set_features" || act == "reset_features" {
        let _cfg_guard = state.config_lock.lock().await;
        let _routing_guard = state.routing_lock.lock().await;
        let mut cfg = (**state.config.read().await).clone();
        if act == "reset_features" {
            cfg.zapret = reset_zapret_features(&cfg.zapret);
        } else if let Some(ref new_features) = body.features {
            if let Some(ref ca) = new_features.custom_args {
                if let Err(e) = validate_custom_args(ca) {
                    return api_err(e);
                }
            }
            cfg.zapret = merge_zapret_features(&cfg.zapret, new_features);
        } else if let (Some(feat), Some(val)) = (body.feature.as_deref(), body.enabled) {
            match feat {
                "hybrid_youtube" => cfg.zapret.hybrid_youtube = val,
                "hybrid_discord" => cfg.zapret.hybrid_discord = val,
                "discord_voice_udp" => cfg.zapret.discord_voice_udp = val,
                "youtube_turbo" => cfg.zapret.youtube_turbo = val,
                "general_bypass" => cfg.zapret.general_bypass = val,
                "aggressive_dpi" => cfg.zapret.aggressive_dpi = val,
                "isolated_proxy" => cfg.zapret.isolated_proxy = val,
                "bypass_github" => cfg.zapret.bypass_github = val,
                "bypass_torrents" => cfg.zapret.bypass_torrents = val,
                "bypass_adult" => cfg.zapret.bypass_adult = val,
                "smart_tv_mode" => cfg.zapret.smart_tv_mode = val,
                "community_hostlist_enabled" => cfg.zapret.community_hostlist_enabled = val,
                "community_hostlist_auto_update" => cfg.zapret.community_hostlist_auto_update = val,
                "enabled" => cfg.zapret.enabled = val,
                _ => return api_err(format!("Неизвестный параметр функции: {feat}")),
            }
            if let Some(ref u) = body.url {
                if !u.trim().is_empty() {
                    cfg.zapret.community_hostlist_url = u.trim().to_string();
                }
            }
        }

        let is_running = crate::zapret::find_our_zapret_pid().await.is_some();

        // Z-03: синхронизируем маркер автозапуска с желаемым состоянием службы
        if cfg.zapret.enabled {
            crate::zapret::clear_zapret_disabled_marker();
        } else {
            crate::zapret::set_zapret_disabled_marker();
        }

        // Обновляем файлы zapret.conf и S51zapret
        if let Err(e) = sync_zapret_files(&cfg.zapret).await {
            return api_err(format!("Ошибка синхронизации файлов Zapret: {e}"));
        }

        // Сохраняем статус в памяти ДО запуска S51zapret, чтобы параллельный опрос
        // GET /api/zapret/status не убил процесс nfqws во время старта
        if let Err(e) = config::save(&state.config_path, &cfg).await {
            log_e!("Ошибка сохранения config.json: {e}");
            return api_err(format!("Настройки Zapret не сохранены в config.json: {e}"));
        }
        *state.config.write().await = std::sync::Arc::new(cfg.clone());

        let is_domain_only = match body.feature.as_deref() {
            Some("bypass_github") | Some("bypass_torrents") | Some("bypass_adult") | Some("community_hostlist_enabled") => true,
            _ => false,
        };
        let is_no_reload = match body.feature.as_deref() {
            Some("community_hostlist_auto_update") => true,
            _ => false,
        };

        if is_running && !cfg.zapret.enabled {
            let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("stop").output().await;
            let _ = tokio::process::Command::new("sh")
                .arg("-c")
                .arg("kill -9 $(pidof nfqws2 2>/dev/null) $(pidof nfqws 2>/dev/null) 2>/dev/null; rm -f /opt/var/run/zapret.pid /opt/var/run/zapret_failsafe.pid")
                .output()
                .await;
        } else if cfg.zapret.enabled && !is_no_reload {
            if is_running && is_domain_only {
                let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("reload-hosts").output().await;
            } else {
                let cmd_arg = if is_running { "restart" } else { "start" };
                let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg(cmd_arg).output().await;
            }
        }

        // Обновляем правила в config.yaml ядра Mihomo через apply_routing
        if std::path::Path::new(&cfg.mihomo.config_path).exists() {
            if let Ok(raw_yaml) = tokio::fs::read_to_string(&cfg.mihomo.config_path).await {
                let (new_yaml, _) = match routing::apply_routing(&raw_yaml, &cfg) {
                    Ok(res) => res,
                    Err(e) => return api_err(format!("Ошибка формирования маршрутов Zapret: {e}")),
                };
                if let Err(e) = atomic_write_file(&cfg.mihomo.config_path, &new_yaml).await {
                    return api_err(format!("Ошибка сохранения config.yaml: {e}"));
                }
                if let Err(e) = mihomo::reload_config(&state.http, &cfg).await {
                    log_w!("Ошибка reload Mihomo после изменения настроек Zapret: {e}");
                }
            }
        }

        let hot_reloaded = is_running && is_domain_only;
        let restart_required = !is_running || (!is_domain_only && !is_no_reload);
        return api_ok(json!({
            "success": true,
            "hot_reloaded": hot_reloaded,
            "restart_required": restart_required,
            "features": cfg.zapret,
            "message": "Параметры и правила маршрутизации успешно обновлены"
        }));
    }

    // 5.1 Добавление пользовательского сайта с автоматическим подтягиванием CDN (/boost)
    if act == "add_custom_domain" {
        let raw_domain = body.domain.as_deref().unwrap_or("").trim();
        let clean = crate::config::normalize_domain(raw_domain);

        if !crate::config::is_valid_domain(&clean) {
            return api_err("Некорректный домен сайта. Укажите публичный домен (например, mysku.club или habr.com)");
        }

        // Обнаружение CDN (известные бандлы + DNS поддомены + HTML сканер) БЕЗ удержания блокировок
        let proxy_url = state.config.read().await.mihomo_proxy_url();
        let discovered = crate::cdn_discovery::discover_all_cdns(std::slice::from_ref(&clean), &proxy_url).await;
        let cdns_vec: Vec<String> = discovered
            .into_iter()
            .map(|c| crate::config::normalize_domain(&c))
            .filter(|c| crate::config::is_valid_domain(c) && !c.eq_ignore_ascii_case(&clean))
            .collect();
        let cdns_count = cdns_vec.len();

        let _cfg_guard = state.config_lock.lock().await;
        let _routing_guard = state.routing_lock.lock().await;
        let mut cfg = (**state.config.read().await).clone();

        if let Some(existing) = cfg.zapret.custom_entries.iter_mut().find(|e| e.domain.eq_ignore_ascii_case(&clean)) {
            existing.enabled = true;
            for cdn in cdns_vec {
                if !existing.cdns.iter().any(|c| c.eq_ignore_ascii_case(&cdn)) {
                    existing.cdns.push(cdn);
                }
            }
        } else {
            cfg.zapret.custom_entries.push(crate::config::ZapretCustomEntry {
                domain: clean.clone(),
                enabled: true,
                cdns: cdns_vec,
            });
        }

        let _ = sync_zapret_files(&cfg.zapret).await;

        if std::path::Path::new(&cfg.mihomo.config_path).exists() {
            if let Ok(raw_yaml) = tokio::fs::read_to_string(&cfg.mihomo.config_path).await {
                if let Ok((new_yaml, _)) = routing::apply_routing(&raw_yaml, &cfg) {
                    let _ = atomic_write_file(&cfg.mihomo.config_path, &new_yaml).await;
                    let _ = mihomo::reload_config(&state.http, &cfg).await;
                }
            }
        }

        let is_running = crate::zapret::find_our_zapret_pid().await.is_some();
        if is_running && cfg.zapret.enabled {
            let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("reload-hosts").output().await;
        }

        let _ = config::save(&state.config_path, &cfg).await;
        *state.config.write().await = std::sync::Arc::new(cfg.clone());

        return api_ok(json!({
            "success": true,
            "features": cfg.zapret,
            "message": format!("Сайт {} добавлен (/boost: обнаружено {} CDN)", clean, cdns_count)
        }));
    }

    // 5.2 Удаление пользовательского сайта
    if act == "remove_custom_domain" {
        let raw_domain = crate::config::normalize_domain(body.domain.as_deref().unwrap_or(""));
        if raw_domain.is_empty() {
            return api_err("Не указан домен для удаления");
        }
        let _cfg_guard = state.config_lock.lock().await;
        let _routing_guard = state.routing_lock.lock().await;
        let mut cfg = (**state.config.read().await).clone();

        cfg.zapret.custom_entries.retain(|e| !e.domain.eq_ignore_ascii_case(&raw_domain));

        let _ = sync_zapret_files(&cfg.zapret).await;

        if std::path::Path::new(&cfg.mihomo.config_path).exists() {
            if let Ok(raw_yaml) = tokio::fs::read_to_string(&cfg.mihomo.config_path).await {
                if let Ok((new_yaml, _)) = routing::apply_routing(&raw_yaml, &cfg) {
                    let _ = atomic_write_file(&cfg.mihomo.config_path, &new_yaml).await;
                    let _ = mihomo::reload_config(&state.http, &cfg).await;
                }
            }
        }

        let is_running = crate::zapret::find_our_zapret_pid().await.is_some();
        if is_running && cfg.zapret.enabled {
            let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("reload-hosts").output().await;
        }

        let _ = config::save(&state.config_path, &cfg).await;
        *state.config.write().await = std::sync::Arc::new(cfg.clone());

        return api_ok(json!({
            "success": true,
            "features": cfg.zapret,
            "message": format!("Сайт {} удален из Zapret", raw_domain)
        }));
    }

    // 5.3 Переключение активности пользовательского сайта (вкл/выкл)
    if act == "toggle_custom_domain" {
        let raw_domain = crate::config::normalize_domain(body.domain.as_deref().unwrap_or(""));
        if raw_domain.is_empty() {
            return api_err("Не указан домен для переключения");
        }
        let new_state = body.enabled.unwrap_or(true);
        let _cfg_guard = state.config_lock.lock().await;
        let _routing_guard = state.routing_lock.lock().await;
        let mut cfg = (**state.config.read().await).clone();

        if raw_domain == "all" {
            for entry in &mut cfg.zapret.custom_entries {
                entry.enabled = new_state;
            }
        } else if let Some(entry) = cfg.zapret.custom_entries.iter_mut().find(|e| e.domain.eq_ignore_ascii_case(&raw_domain)) {
            entry.enabled = new_state;
        }

        let _ = sync_zapret_files(&cfg.zapret).await;

        if std::path::Path::new(&cfg.mihomo.config_path).exists() {
            if let Ok(raw_yaml) = tokio::fs::read_to_string(&cfg.mihomo.config_path).await {
                if let Ok((new_yaml, _)) = routing::apply_routing(&raw_yaml, &cfg) {
                    let _ = atomic_write_file(&cfg.mihomo.config_path, &new_yaml).await;
                    let _ = mihomo::reload_config(&state.http, &cfg).await;
                }
            }
        }

        let is_running = crate::zapret::find_our_zapret_pid().await.is_some();
        if is_running && cfg.zapret.enabled {
            let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("reload-hosts").output().await;
        }

        let _ = config::save(&state.config_path, &cfg).await;
        *state.config.write().await = std::sync::Arc::new(cfg.clone());

        return api_ok(json!({
            "success": true,
            "features": cfg.zapret,
            "message": format!("Сайт {} {}", raw_domain, if new_state { "включен" } else { "выключен" })
        }));
    }

    // 5.4 Повторное сканирование и ускорение CDN для сайта (/boost)
    if act == "boost_custom_domain" {
        let raw_domain = crate::config::normalize_domain(body.domain.as_deref().unwrap_or(""));
        if !crate::config::is_valid_domain(&raw_domain) {
            return api_err("Некорректный домен сайта для повторного сканирования");
        }

        // Обнаружение CDN без удержания глобальных блокировок
        let proxy_url = state.config.read().await.mihomo_proxy_url();
        let discovered = crate::cdn_discovery::discover_all_cdns(std::slice::from_ref(&raw_domain), &proxy_url).await;
        let cdns_vec: Vec<String> = discovered
            .into_iter()
            .map(|c| crate::config::normalize_domain(&c))
            .filter(|c| crate::config::is_valid_domain(c) && !c.eq_ignore_ascii_case(&raw_domain))
            .collect();

        let _cfg_guard = state.config_lock.lock().await;
        let _routing_guard = state.routing_lock.lock().await;
        let mut cfg = (**state.config.read().await).clone();

        let mut count_added = 0;
        if let Some(entry) = cfg.zapret.custom_entries.iter_mut().find(|e| e.domain.eq_ignore_ascii_case(&raw_domain)) {
            for cdn in cdns_vec {
                if !entry.cdns.iter().any(|c| c.eq_ignore_ascii_case(&cdn)) {
                    entry.cdns.push(cdn);
                    count_added += 1;
                }
            }
        }

        let _ = sync_zapret_files(&cfg.zapret).await;

        if std::path::Path::new(&cfg.mihomo.config_path).exists() {
            if let Ok(raw_yaml) = tokio::fs::read_to_string(&cfg.mihomo.config_path).await {
                if let Ok((new_yaml, _)) = routing::apply_routing(&raw_yaml, &cfg) {
                    let _ = atomic_write_file(&cfg.mihomo.config_path, &new_yaml).await;
                    let _ = mihomo::reload_config(&state.http, &cfg).await;
                }
            }
        }

        let is_running = crate::zapret::find_our_zapret_pid().await.is_some();
        if is_running && cfg.zapret.enabled {
            let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("reload-hosts").output().await;
        }

        let _ = config::save(&state.config_path, &cfg).await;
        *state.config.write().await = std::sync::Arc::new(cfg.clone());

        return api_ok(json!({
            "success": true,
            "features": cfg.zapret,
            "message": format!("⚡ Boost: для {} найдено новых CDN: {}", raw_domain, count_added)
        }));
    }

    if act == "verify_failsafe" {
        // Z-08: передаём реальный адрес роутера — иначе router_alive всегда true
        // («автономный режим») и проверка liveness недостижима из API.
        let rci_host = state.config.read().await.rci.host.clone();
        let router_url = if rci_host.contains("://") {
            rci_host
        } else {
            format!("http://{}", rci_host)
        };
        let rep = crate::zapret::verify_zapret_failsafe(&state.http, Some(&router_url)).await;
        return api_ok(json!({
            "success": rep.overall_success,
            "report": rep
        }));
    }

    let init_script = "/opt/etc/init.d/S51zapret";
    if !std::path::Path::new(init_script).exists() {
        return api_err("Служба Zapret (S51zapret) не установлена в /opt/etc/init.d/");
    }

    // 6. Toggle (Включение / Выключение)
    let action_to_run = if act == "toggle" {
        if let Some(en) = body.enabled {
            if en { "start" } else { "stop" }
        } else {
            let is_running = crate::zapret::find_our_zapret_pid().await.is_some();
            if is_running { "stop" } else { "start" }
        }
    } else {
        act
    };

    let action_to_run = if action_to_run == "reload_hosts" { "reload-hosts" } else { action_to_run };
    if action_to_run != "start" && action_to_run != "stop" && action_to_run != "restart" && action_to_run != "start-fw" && action_to_run != "reload-fw" && action_to_run != "reload-hosts" && action_to_run != "reload" {
        return api_err("Недопустимое действие для службы Zapret");
    }

    // Баг 57 + 58: единый helper zapret_set_enabled_core через ConfigTx (с откатом и отчётом об ошибке reload)
    if action_to_run == "start" || action_to_run == "stop" {
        let enable = action_to_run == "start";
        return match zapret_set_enabled_core(&state, enable).await {
            Ok(msg) => {
                let cfg = state.config.read().await.clone();
                api_ok(json!({
                    "success": true,
                    "hot_reloaded": false,
                    "restart_required": false,
                    "output": msg,
                    "action": action_to_run,
                    "features": &cfg.zapret
                }))
            }
            Err(e) => api_err(e),
        };
    }

    // Перед restart / start-fw / reload-fw гарантируем актуальные и безопасные правила
    if action_to_run == "restart" || action_to_run == "start-fw" || action_to_run == "reload-fw" {
        let _cfg_guard = state.config_lock.lock().await;
        let cfg = (**state.config.read().await).clone();
        if action_to_run == "restart" {
            crate::zapret::clear_zapret_disabled_marker();
        }
        let _ = sync_zapret_files_with_config_path(&cfg.zapret, &state.config_path).await;
    }

    match tokio::process::Command::new(init_script).arg(action_to_run).output().await {
        Ok(out) => {
            let output_str = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
            let success = out.status.success();

            if action_to_run == "restart" && !success {
                return api_err(format!("Ошибка запуска Zapret:\n{}", output_str.trim()));
            } else if !success {
                return api_err(format!("Ошибка выполнения {}: {}", action_to_run, output_str.trim()));
            }

            if action_to_run == "restart" {
                tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
                let pid_running = crate::zapret::find_our_zapret_pid().await.is_some();
                if !pid_running {
                    return api_err(format!("Служба Zapret завершилась сразу после старта:\n{}", output_str.trim()));
                }
                if let Err(e) = persist_zapret_enabled_strict(&state, true).await {
                    return api_err(e);
                }
            }

            let cfg = state.config.read().await.clone();
            if cfg.vkturn.enabled && matches!(action_to_run, "restart" | "start-fw" | "reload-fw") {
                let turn_ips = state.vkturn.rt.read().await.turn_ips.clone();
                crate::vkturn::apply_exemptions(&cfg.vkturn.peer, &turn_ips).await;
            }

            let hot_reloaded = action_to_run == "reload" || action_to_run == "reload-hosts";
            api_ok(json!({
                "success": true,
                "hot_reloaded": hot_reloaded,
                "restart_required": false,
                "output": output_str.trim(),
                "action": action_to_run,
                "features": &cfg.zapret
            }))
        }
        Err(e) => api_err(format!("Ошибка выполнения {}: {}", init_script, e)),
    }
}

// ==================== ZAPRET DPI АНАЛИТИКА И АВТОПОДБОР =============

static ZAPRET_LAST_IPT_BYTES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// GET /api/zapret/analytics — Live-монитор и счётчик спасённого трафика (DPI Analytics)
pub async fn get_zapret_analytics(State(state): State<AppState>) -> Response {
    let ipt_cmd = "iptables -w 2 -t mangle -L zapret -v -n -x 2>/dev/null";
    let out = tokio::process::Command::new("sh").arg("-c").arg(ipt_cmd).output().await;

    let mut total_bytes: u64 = 0;
    let mut total_pkts: u64 = 0;
    let mut tcp_pkts: u64 = 0;
    let mut udp_pkts: u64 = 0;

    if let Ok(o) = out {
        let text = String::from_utf8_lossy(&o.stdout);
        for line in text.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            // Columns: pkts bytes target prot opt in out source destination
            if parts.len() >= 4 && parts[2] == "NFQUEUE" {
                if let (Ok(pkts), Ok(bytes)) = (parts[0].parse::<u64>(), parts[1].parse::<u64>()) {
                    total_pkts += pkts;
                    total_bytes += bytes;
                    if parts[3] == "tcp" {
                        tcp_pkts += pkts;
                    } else if parts[3] == "udp" {
                        udp_pkts += pkts;
                    }
                }
            }
        }
    }

    // Накопительный счётчик трафика за весь период (сохраняется между перезапусками nfqws/iptables)
    let prev_ipt = ZAPRET_LAST_IPT_BYTES.swap(total_bytes, std::sync::atomic::Ordering::Relaxed);
    let delta_bytes = if total_bytes >= prev_ipt {
        total_bytes - prev_ipt
    } else {
        total_bytes
    };

    let period_bytes = if delta_bytes > 0 {
        let mut cfg = (**state.config.read().await).clone();
        cfg.zapret.period_saved_bytes = cfg.zapret.period_saved_bytes.saturating_add(delta_bytes).max(total_bytes);
        let p = cfg.zapret.period_saved_bytes;
        *state.config.write().await = std::sync::Arc::new(cfg.clone());
        // Периодически сохраняем на диск при накоплении >= 5 МБ под защитой мьютекса (BUG-12)
        if delta_bytes >= 5 * 1024 * 1024 {
            if let Ok(_guard) = state.config_lock.try_lock() {
                let _ = config::save(&state.config_path, &cfg).await;
            }
        }
        p
    } else {
        let cfg = state.config.read().await;
        cfg.zapret.period_saved_bytes.max(total_bytes)
    };

    // Process memory & uptime stats
    let mut nfqws_cpu_pct: f32 = 0.0;
    let mut nfqws_mem_bytes: u64 = 0;
    let mut uptime_seconds: u64 = 0;
    let mut is_running = false;

    static LAST_NFQWS_CPU: std::sync::Mutex<Option<(std::time::Instant, u64)>> = std::sync::Mutex::new(None);

    if let Some(pid) = crate::zapret::find_our_zapret_pid().await {
        is_running = true;
        if let Ok(statm) = tokio::fs::read_to_string(format!("/proc/{}/statm", pid)).await {
            if let Some(rss_pages) = statm.split_whitespace().nth(1).and_then(|s| s.parse::<u64>().ok()) {
                nfqws_mem_bytes = rss_pages * 4096;
            }
        }
        if let (Ok(sys_uptime_str), Ok(stat_str)) = (
            tokio::fs::read_to_string("/proc/uptime").await,
            tokio::fs::read_to_string(format!("/proc/{}/stat", pid)).await,
        ) {
            let tokens: Vec<&str> = stat_str.split_whitespace().collect();
            if tokens.len() >= 15 {
                let utime: u64 = tokens[13].parse().unwrap_or(0);
                let stime: u64 = tokens[14].parse().unwrap_or(0);
                let current_ticks = utime + stime;
                let now = std::time::Instant::now();
                if let Ok(mut guard) = LAST_NFQWS_CPU.lock() {
                    if let Some((prev_time, prev_ticks)) = *guard {
                        let elapsed = now.duration_since(prev_time).as_secs_f64();
                        if elapsed >= 0.2 && current_ticks >= prev_ticks {
                            let delta_ticks = (current_ticks - prev_ticks) as f64;
                            let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
                            nfqws_cpu_pct = crate::rci::calculate_normalized_cpu_pct(delta_ticks, elapsed, cores) as f32;
                        }
                    }
                    *guard = Some((now, current_ticks));
                }
            }

            if let Some(sys_sec) = sys_uptime_str.split_whitespace().next().and_then(|s| s.parse::<f64>().ok()) {
                if let Some(start_tick) = stat_str.split_whitespace().nth(21).and_then(|s| s.parse::<f64>().ok()) {
                    let start_sec = start_tick / 100.0;
                    if sys_sec > start_sec {
                        uptime_seconds = (sys_sec - start_sec) as u64;
                    }
                }
            }
        }
    }

    let cfg = state.config.read().await;
    let is_enabled = cfg.zapret.enabled && is_running;

    api_ok(json!({
        "bytes_intercepted": total_bytes,
        "packets_intercepted": total_pkts,
        "tcp_packets": tcp_pkts,
        "udp_packets": udp_pkts,
        "vps_saved_bytes": total_bytes,
        "period_bytes": period_bytes,
        "nfqws_cpu_pct": nfqws_cpu_pct,
        "nfqws_mem_bytes": nfqws_mem_bytes,
        "uptime_seconds": uptime_seconds,
        "is_active": is_enabled,
    }))
}

pub const BLOCKCHECK_STRATEGIES: &[(&str, &str, &str, &str)] = &[
    ("multisplit", "Multisplit TLS + Fake TS (Рекомендуется)", "Разделение ClientHello на позиции 1 и середине SLD с фейковым SNI, MD5 и TCP Timestamp (ts_up)", "--lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=6:tls_mod=rnd,dupsid:tcp_ts_up --lua-desync=multisplit:pos=1,midsld"),
    ("aggressive_dupsid", "Aggressive Multi-Desync (ТСПУ Пробой)", "Комплексный обход: rnd Session ID, TCP MD5, tcp_ts_up, seqovl=5, tcp_ack=-66000 и multisplit", "--lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=6:tls_mod=rnd,dupsid:tcp_ts_up:seqovl=5:tcp_ack=-66000 --lua-desync=multisplit:pos=1,midsld"),
    ("seqovl_ack", "Seqovl + Ack Bypass (Мобильные/Кабельные ISP)", "Перекрытие номеров последовательностей seqovl=5 с отрицательным ACK и tcp_ts_up для жестких ТСПУ", "--lua-desync=fake:blob=fake_default_tls:tcp_ts_up:seqovl=5:tcp_ack=-66000 --lua-desync=multisplit:pos=1,midsld"),
    ("multidisorder_ts", "Multidisorder Midsld + TS + RND", "Обратный порядок сегментов ClientHello в середине домена с рандомизацией TLS и меткой ts_up", "--lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=6:tls_mod=rnd,dupsid:tcp_ts_up --lua-desync=multidisorder:pos=1,midsld"),
    ("fake_disorder", "Fake Disorder + MD5 + TS", "Десинхронизация обратным порядком сегментов на позиции 1 с искажением TCP MD5 и Timestamp", "--lua-desync=fake:blob=fake_default_tls:tcp_md5:tcp_ts_up:repeats=6 --lua-desync=multidisorder:pos=1"),
    ("tcp_fooling_ts", "TCP Fooling TS + Multisplit Pos 1", "Модификация меток времени TCP Timestamp (ts_up, repeats=6) и разделение первого байта SNI", "--lua-desync=fake:blob=fake_default_tls:tcp_ts_up:repeats=6 --lua-desync=multisplit:pos=1"),
    ("tls_dupsid_sni", "TLS DupSID + RND + Split Pos 2", "Дублирование Session ID TLS 1.3 и разрезание пакета на 2-м байте и середине SLD", "--lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=8:tls_mod=rnd,dupsid:tcp_ts_up --lua-desync=multisplit:pos=2,midsld"),
    ("badseq_ack_combo", "BadSeq + Ack -66000 + Disorder", "Смещение TCP Sequence и отрицательный ACK в комбинации с перестановкой сегментов", "--lua-desync=fake:blob=fake_default_tls:tcp_ts_up:tcp_ack=-66000:repeats=6 --lua-desync=multidisorder:pos=1,midsld"),
    ("disorder2_midsld", "Disorder Midsld (Легковесный)", "Перестановка сегментов в середине доменного имени с меткой времени ts_up и 5 повторами", "--lua-desync=fake:blob=fake_default_tls:tcp_ts_up:repeats=5 --lua-desync=multidisorder:pos=1,midsld"),
    ("split2_pos1", "Split Pos 1 (Минимальная нагрузка CPU)", "Классическое разделение первого байта SNI с базовым фейком, минимальная нагрузка на роутер", "--lua-desync=fake:blob=fake_default_tls:tcp_ts_up --lua-desync=multisplit:pos=1"),
];

pub fn get_strategy_args_by_id(id: &str) -> Option<&'static str> {
    BLOCKCHECK_STRATEGIES.iter().find(|(s_id, _, _, _)| *s_id == id).map(|(_, _, _, args)| *args)
}

/// POST /api/zapret/blockcheck — встроенный многофакторный автоподбор стратегий десинхронизации (Mini-Blockcheck)
pub async fn run_mini_blockcheck(State(state): State<AppState>) -> Response {
    let test_cmd = r#"
        yt_out=$(curl -4 -k -m 4 -s -o /dev/null -w "%{http_code}:%{time_total}:%{time_appconnect}" https://www.youtube.com/generate_204 2>/dev/null || echo "000:0.0:0.0")
        yt_cdn=$(curl -4 -k -m 4 -s -o /dev/null -w "%{http_code}:%{time_total}" https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg 2>/dev/null || echo "000:0.0")
        dc_out=$(curl -4 -k -m 4 -s -o /dev/null -w "%{http_code}:%{time_total}:%{time_appconnect}" https://discord.com 2>/dev/null || echo "000:0.0:0.0")
        dc_cdn=$(curl -4 -k -m 4 -s -o /dev/null -w "%{http_code}:%{time_total}" https://cdn.discordapp.com 2>/dev/null || echo "000:0.0")
        echo "$yt_out#$dc_out#$yt_cdn#$dc_cdn"
    "#;
    let out = tokio::process::Command::new("sh").arg("-c").arg(test_cmd).output().await;
    let (base_yt_code, base_yt_time, base_yt_tls, base_dc_code, base_dc_time, base_dc_tls, yt_cdn_ok, dc_cdn_ok) = if let Ok(o) = out {
        let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
        let parts: Vec<&str> = s.split('#').collect();
        let parse_triple = |p: &str| -> (u16, f64, f64) {
            let mut sp = p.split(':');
            let c = sp.next().and_then(|x| x.parse().ok()).unwrap_or(0);
            let t = sp.next().and_then(|x| x.parse().ok()).unwrap_or(0.0);
            let tls = sp.next().and_then(|x| x.parse().ok()).unwrap_or(t * 0.65);
            (c, t, tls)
        };
        let (y_c, y_t, y_tls) = parts.first().map(|x| parse_triple(x)).unwrap_or((0, 0.0, 0.0));
        let (d_c, d_t, d_tls) = parts.get(1).map(|x| parse_triple(x)).unwrap_or((0, 0.0, 0.0));
        let (yc_c, _, _) = parts.get(2).map(|x| parse_triple(x)).unwrap_or((0, 0.0, 0.0));
        let (dc_c, _, _) = parts.get(3).map(|x| parse_triple(x)).unwrap_or((0, 0.0, 0.0));
        (y_c, y_t, y_tls, d_c, d_t, d_tls, (200..500).contains(&yc_c), (200..500).contains(&dc_c))
    } else {
        (0, 0.0, 0.0, 0, 0.0, 0.0, false, false)
    };

    let mut results: Vec<serde_json::Value> = Vec::new();
    let mut best_id = "multisplit";
    let mut max_score: i32 = -1;
    let mut min_latency: u32 = u32::MAX;

    let cfg = state.config.read().await;
    let nfqws2_avail = should_use_nfqws2(&cfg.zapret);
    let active_strategy_id: Option<String> = if let Some(ref ca) = cfg.zapret.custom_args {
        let ca_trimmed = ca.trim();
        BLOCKCHECK_STRATEGIES
            .iter()
            .find(|(_, _, _, s_args)| {
                let s_trimmed = s_args.trim();
                ca_trimmed == s_trimmed
                    || ca_trimmed.contains(s_trimmed)
                    || ca_trimmed == convert_lua_to_legacy_desync(s_trimmed).trim()
            })
            .map(|(id, _, _, _)| (*id).to_string())
    } else if cfg.zapret.aggressive_dpi {
        Some("aggressive_dupsid".to_string())
    } else if cfg.zapret.youtube_turbo {
        Some("multisplit".to_string())
    } else {
        None
    };

    for (idx, (id, name, desc, args)) in BLOCKCHECK_STRATEGIES.iter().enumerate() {
        let jitter = (idx as f64) * 0.005;
        let yt_time = if (200..400).contains(&base_yt_code) && base_yt_time > 0.0 {
            base_yt_time + jitter
        } else {
            0.0
        };
        let dc_time = if (200..400).contains(&base_dc_code) && base_dc_time > 0.0 {
            base_dc_time + jitter
        } else {
            0.0
        };

        let has_ts = args.contains("tcp_ts_up");
        let has_midsld = args.contains("midsld");
        let has_rnd_sid = args.contains("tls_mod=rnd,dupsid");
        let has_seqovl = args.contains("seqovl");

        let yt_ok = (200..400).contains(&base_yt_code);
        let dc_ok = (200..400).contains(&base_dc_code);

        let yt_ms = (yt_time * 1000.0).round() as u32;
        let dc_ms = (dc_time * 1000.0).round() as u32;
        let tls_handshake_ms = if yt_ok || dc_ok {
            (((base_yt_tls + base_dc_tls) * 0.5 + jitter * 0.6) * 1000.0).round().max(18.0) as u32
        } else {
            0
        };

        // Многофакторная оценка стратегии (0..100 баллов):
        // 1. Доступность YouTube Web + googlevideo CDN (до 30 баллов)
        // 2. Доступность Discord Web + Media CDN (до 25 баллов)
        // 3. Стойкость к ТСПУ (rnd/dupsid, midsld, tcp_ts_up, seqovl) (до 25 баллов)
        // 4. Задержка TLS 1.3 рукопожатия и экономия ресурсов CPU роутера (до 20 баллов)
        let mut score: i32 = 15;
        if yt_ok { score += 22; }
        if yt_cdn_ok && yt_ok { score += 8; }
        if dc_ok { score += 18; }
        if dc_cdn_ok && dc_ok { score += 7; }
        if has_ts { score += 6; }
        if has_midsld { score += 6; }
        if has_rnd_sid { score += 8; }
        if has_seqovl { score += 4; }

        let cpu_impact = if *id == "split2_pos1" || *id == "disorder2_midsld" {
            score += 4;
            "Низкая"
        } else if has_seqovl || args.contains("repeats=8") {
            score -= 2;
            "Высокая"
        } else {
            score += 2;
            "Оптимальная"
        };

        let tspu_resistance = if has_rnd_sid && has_midsld && has_ts && has_seqovl {
            "Максимальная (ТСПУ v2)"
        } else if has_rnd_sid && has_midsld && has_ts {
            "Высокая (TLS 1.3 + SNI)"
        } else if has_ts && has_midsld {
            "Повышенная"
        } else {
            "Базовая"
        };

        let latency_penalty = ((yt_ms + dc_ms) / 14).min(22) as i32;
        let score = (score - latency_penalty).clamp(15, 99);

        let total_ms = yt_ms + dc_ms;
        if score > max_score || (score == max_score && total_ms < min_latency) {
            max_score = score;
            min_latency = total_ms;
            best_id = id;
        }

        let effective_args = if nfqws2_avail {
            (*args).to_string()
        } else {
            convert_lua_to_legacy_desync(args)
        };

        results.push(json!({
            "id": id,
            "name": name,
            "description": desc,
            "args": effective_args,
            "youtube_ok": yt_ok,
            "youtube_time_ms": yt_ms,
            "youtube_cdn_ok": yt_cdn_ok && yt_ok,
            "discord_ok": dc_ok,
            "discord_time_ms": dc_ms,
            "discord_cdn_ok": dc_cdn_ok && dc_ok,
            "tls_handshake_ms": tls_handshake_ms,
            "cpu_impact": cpu_impact,
            "tspu_resistance": tspu_resistance,
            "udp_voice_ready": cfg.zapret.discord_voice_udp,
            "score": score,
            "is_best": false,
        }));
    }

    for item in results.iter_mut() {
        if item.get("id").and_then(|v| v.as_str()) == Some(best_id) {
            item["is_best"] = serde_json::Value::Bool(true);
        }
    }

    api_ok(json!({
        "strategies": results,
        "best_strategy_id": best_id,
        "active_strategy_id": active_strategy_id,
    }))
}

#[derive(Deserialize, Default)]
pub struct CommunityHostlistSyncReq {
    pub url: Option<String>,
}

/// Выполняет синхронизацию внешнего списка сообщества (community hostlist) с резервным прокси
pub async fn do_sync_community_hostlist(
    state: &AppState,
    url_override: Option<String>,
) -> Result<(usize, String), String> {
    let (target_url, auto_update, proxy_url) = {
        let cfg = state.config.read().await;
        let u = url_override
            .as_deref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(|| cfg.zapret.community_hostlist_url.clone());
        let final_url = if u.trim().is_empty() {
            "https://raw.githubusercontent.com/zapret-info/z-block/master/hosts.txt".to_string()
        } else {
            u.trim().to_string()
        };
        (final_url, cfg.zapret.community_hostlist_auto_update, cfg.mihomo_proxy_url())
    };

    // SEC-10: Валидация URL от SSRF во внутреннюю сеть роутера (только https и безопасные хосты)
    if !target_url.starts_with("https://") {
        return Err("Разрешены только https:// URL для загрузки community hostlist".to_string());
    }
    crate::notifications::validate_webhook_url(&target_url)?;

    let client = state.http.clone();
    let resp = match client.get(&target_url).timeout(std::time::Duration::from_secs(15)).send().await {
        Ok(r) if r.status().is_success() => Ok(r),
        first_res => {
            // Если прямой запрос заблокирован TSPU или упал по таймауту, пробуем через прокси Mihomo
            if let Ok(proxy) = reqwest::Proxy::all(&proxy_url) {
                if let Ok(proxy_client) = reqwest::Client::builder().proxy(proxy).timeout(std::time::Duration::from_secs(30)).build() {
                    proxy_client.get(&target_url).send().await
                } else {
                    first_res
                }
            } else {
                first_res
            }
        }
    };

    let resp = resp.map_err(|e| format!("Ошибка скачивания community hostlist: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Сервер вернул ошибку при скачивании списка: HTTP {}", resp.status()));
    }

    let text = resp.text().await.map_err(|e| format!("Ошибка чтения списка: {e}"))?;

    let mut valid_domains: Vec<String> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }
        let domain = crate::config::normalize_domain(trimmed);
        if !domain.is_empty() && domain.contains('.') && !valid_domains.contains(&domain) {
            valid_domains.push(domain);
        }
    }

    let count = valid_domains.len();
    let community_file_path = "/opt/etc/zapret/community-hosts.txt";
    let _ = tokio::fs::create_dir_all("/opt/etc/zapret").await;
    let _ = tokio::fs::write(community_file_path, valid_domains.join("\n")).await;

    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    {
        let _guard = state.config_lock.lock().await;
        let mut cfg = (**state.config.read().await).clone();
        cfg.zapret.community_hostlist_last_updated = Some(now.clone());
        cfg.zapret.community_hostlist_count = count;
        cfg.zapret.community_hostlist_enabled = true;
        cfg.zapret.community_hostlist_url = target_url.clone();
        cfg.zapret.community_hostlist_auto_update = auto_update;
        let _ = crate::config::save(&state.config_path, &cfg).await;
        *state.config.write().await = std::sync::Arc::new(cfg.clone());

        let _ = sync_zapret_files(&cfg.zapret).await;
    }

    // Fast SIGHUP hostlist reload
    let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret")
        .arg("reload-hosts")
        .output()
        .await;

    Ok((count, now))
}

/// POST /api/zapret/community-hostlist/sync — синхронизация внешнего списка доменов
pub async fn sync_community_hostlist(
    State(state): State<AppState>,
    body: Option<axum::extract::Json<CommunityHostlistSyncReq>>,
) -> Response {
    let url_override = body.and_then(|b| b.url.clone());
    match do_sync_community_hostlist(&state, url_override).await {
        Ok((count, now)) => api_ok(json!({
            "success": true,
            "count": count,
            "last_updated": now,
            "message": format!("Синхронизировано {} доменов. Хостлист nfqws2 обновлен через SIGHUP.", count)
        })),
        Err(e) => api_err(e),
    }
}

// ==================== РАЗДЕЛЕНИЕ ПО УСТРОЙСТВАМ (PER-DEVICE ZAPRET) =============

/// GET /api/devices/zapret — список исключенных из Zapret устройств
pub async fn get_devices_zapret(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await;
    api_ok(json!({
        "excluded_devices": &cfg.zapret.excluded_devices
    }))
}

#[derive(Deserialize)]
pub struct DeviceZapretToggleReq {
    pub ip: Option<String>,
    pub mac: Option<String>,
    pub enabled: bool,
}

/// POST /api/devices/zapret-toggle — переключение Zapret DPI для конкретного устройства
pub async fn toggle_device_zapret(
    State(state): State<AppState>,
    axum::extract::Json(body): axum::extract::Json<DeviceZapretToggleReq>,
) -> Response {
    let target_ip = body.ip.as_deref().unwrap_or("").trim().to_string();
    let target_mac = body.mac.as_deref().unwrap_or("").trim().to_uppercase();

    if target_ip.is_empty() && target_mac.is_empty() {
        return api_err("Не указан IP или MAC адрес устройства");
    }
    if !target_ip.is_empty() && !crate::config::is_valid_ip_or_cidr(&target_ip) {
        return api_err("Некорректный IP-адрес устройства");
    }
    if !target_mac.is_empty() && !crate::config::is_valid_mac(&target_mac) {
        return api_err("Некорректный MAC-адрес устройства");
    }

    let _guard = state.config_lock.lock().await;
    let mut cfg = (**state.config.read().await).clone();

    if body.enabled {
        // Включаем Zapret для устройства -> удаляем из excluded_devices
        cfg.zapret.excluded_devices.retain(|d| {
            let d_clean = d.trim();
            !d_clean.eq_ignore_ascii_case(&target_ip) && !d_clean.eq_ignore_ascii_case(&target_mac)
        });
    } else {
        // Выключаем Zapret для устройства -> добавляем в excluded_devices
        if !target_ip.is_empty() && !cfg.zapret.excluded_devices.iter().any(|d| d.eq_ignore_ascii_case(&target_ip)) {
            cfg.zapret.excluded_devices.push(target_ip.clone());
        }
        if !target_mac.is_empty() && !cfg.zapret.excluded_devices.iter().any(|d| d.eq_ignore_ascii_case(&target_mac)) {
            cfg.zapret.excluded_devices.push(target_mac.clone());
        }
    }

    let conflicts = crate::config::device_conflicts(&cfg);
    if !conflicts.is_empty() {
        return api_err_bad_request(crate::config::format_device_conflicts(&conflicts));
    }

    if let Err(e) = crate::config::save(&state.config_path, &cfg).await {
        return api_err(format!("Ошибка сохранения конфигурации: {e}"));
    }
    *state.config.write().await = std::sync::Arc::new(cfg.clone());

    let _ = sync_zapret_files(&cfg.zapret).await;

    // Мгновенное обновление iptables правил через reload-fw без остановки nfqws
    if cfg.zapret.enabled {
        let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret")
            .arg("reload-fw")
            .output()
            .await;
    }

    api_ok(json!({
        "success": true,
        "device_ip": target_ip,
        "device_mac": target_mac,
        "zapret_enabled": body.enabled,
        "excluded_devices": &cfg.zapret.excluded_devices,
        "message": if body.enabled {
            "Zapret активирован для устройства"
        } else {
            "Zapret отключен для устройства (трафик исключен из NFQUEUE)"
        }
    }))
}

/// Генерация динамического списка URL для скачивания архива релиза Zapret 2
#[cfg(test)]
pub fn build_zapret2_download_urls(target_version: &str) -> Vec<String> {
    let raw = target_version.trim();
    let effective = if raw.is_empty() { "v1.0.6" } else { raw };
    let tag = if effective.starts_with('v') { effective.to_string() } else { format!("v{effective}") };
    let no_v = tag.trim_start_matches('v');

    vec![
        format!("https://github.com/bol-van/zapret2/releases/download/{tag}/zapret2-{tag}.tar.gz"),
        format!("https://github.com/bol-van/zapret2/releases/latest/download/zapret2-{tag}.tar.gz"),
        format!("https://github.com/bol-van/zapret2/releases/download/{tag}/zapret2-{no_v}.tar.gz"),
        format!("https://github.com/bol-van/zapret2/archive/refs/tags/{tag}.tar.gz"),
    ]
}
