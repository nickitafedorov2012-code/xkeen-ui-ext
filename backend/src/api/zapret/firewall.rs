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

pub(super) fn validate_port_token(token: &str) -> Result<String, String> {
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
if command iptables -w -L -n >/dev/null 2>&1; then
  IPTABLES_CMD="iptables -w"
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
  while iptables -t nat -D PREROUTING -i br+ -p udp --dport 53 -m comment --comment "xkeen-route-zapret" -j REDIRECT --to-ports __ZAPRET_DNS_PORT__ 2>/dev/null; do :; done
  while iptables -t nat -D PREROUTING -i br+ -p tcp --dport 53 -m comment --comment "xkeen-route-zapret" -j REDIRECT --to-ports __ZAPRET_DNS_PORT__ 2>/dev/null; do :; done
  while iptables -t nat -D PREROUTING -i Bridge+ -p udp --dport 53 -m comment --comment "xkeen-route-zapret" -j REDIRECT --to-ports __ZAPRET_DNS_PORT__ 2>/dev/null; do :; done
  while iptables -t nat -D PREROUTING -i Bridge+ -p tcp --dport 53 -m comment --comment "xkeen-route-zapret" -j REDIRECT --to-ports __ZAPRET_DNS_PORT__ 2>/dev/null; do :; done


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

