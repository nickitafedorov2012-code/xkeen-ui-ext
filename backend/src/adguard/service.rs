use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;
use crate::config::AdGuardConfig;
use super::{agh_request, get_status};
use super::iptables::is_adguard_iptables_active;
use super::yaml::{detect_agh_loop_risk, ensure_default_adguard_config, find_adguard_config};
#[cfg(target_os = "linux")]
use super::{
    apply_adguard_iptables, detect_adguard_dns_port, remove_adguard_iptables,
    sanitize_and_heal_adguard_yaml,
};

/// Тестирование локального UDP DNS слушателя AdGuard Home
pub async fn test_agh_dns_listener(host: &str, port: u16) -> bool {
    let addr = match format!("{}:{}", host, port).parse::<SocketAddr>() {
        Ok(a) => a,
        Err(_) => return false,
    };

    let bind_addr = if addr.is_ipv6() { "[::]:0" } else { "0.0.0.0:0" };
    let sock = match tokio::net::UdpSocket::bind(bind_addr).await {
        Ok(s) => s,
        Err(_) => return false,
    };

    if sock.connect(addr).await.is_err() {
        return false;
    }

    // DNS-запрос для probe.test (IN A)
    let query: [u8; 28] = [
        0xAD, 0x64, // TxID
        0x01, 0x00, // Flags: standard query, RD=1
        0x00, 0x01, // QDCOUNT: 1
        0x00, 0x00, // ANCOUNT: 0
        0x00, 0x00, // NSCOUNT: 0
        0x00, 0x00, // ARCOUNT: 0
        0x05, b'p', b'r', b'o', b'b', b'e',
        0x04, b't', b'e', b's', b't',
        0x00,       // null label
        0x00, 0x01, // QTYPE: A
        0x00, 0x01, // QCLASS: IN
    ];

    if sock.send(&query).await.is_err() {
        return false;
    }

    let mut buf = [0u8; 512];
    match tokio::time::timeout(Duration::from_millis(2500), sock.recv(&mut buf)).await {
        Ok(Ok(n)) => crate::zapret::validate_dns_response((0xAD, 0x64), &buf[..n]),
        _ => false,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AghDiagnostics {
    pub port_53_status: String,
    pub port_3000_status: String,
    pub dnsmasq_redirect_active: bool,
    pub iptables_redirect_active: bool,
    pub detected_service_path: Option<String>,
    pub detected_config_path: Option<String>,
    pub loop_risk: bool,
    pub recommendations: Vec<String>,
}

pub const AGH_IPTABLES_CHAIN: &str = "XKEEN_ADGUARD";
pub const AGH_LAN_INTERFACES: &[&str] = &["br+", "Bridge+", "ra+"];
pub const DEFAULT_ADGUARD_PORT: u16 = 5353;
pub const DEFAULT_ADGUARD_DNS_FALLBACK_PORT: u16 = 5354;

/// Проверка активности процесса AdGuardHome на роутере (на не-Linux платформах возвращает true для dev/test).
pub fn is_adguard_process_alive() -> bool {
    #[cfg(target_os = "linux")]
    {
        crate::system::is_process_running("AdGuardHome")
    }
    #[cfg(not(target_os = "linux"))]
    {
        true
    }
}

/// Запуск комплексной диагностики интеграции AdGuard Home
pub async fn get_diagnostics(
    http: &reqwest::Client,
    cfg: &AdGuardConfig,
    router_ip: Option<&str>,
) -> AghDiagnostics {
    let mut recommendations = Vec::new();

    // 1. Проверка доступности портов
    let port_53_active = test_agh_dns_listener(&cfg.host, cfg.dns_port).await;
    let port_53_status = if port_53_active {
        "active".to_string()
    } else {
        "inactive".to_string()
    };

    let status = get_status(http, cfg).await;
    let port_3000_status = if status.running {
        "active".to_string()
    } else {
        "inactive".to_string()
    };

    // 2. Обнаружение файлов конфигурации и службы
    let detected_service_path = find_adguard_init_script().map(|p| p.to_string_lossy().to_string());
    let detected_config_path = find_adguard_config().map(|p| p.to_string_lossy().to_string());
    let detected_binary_path = find_adguard_binary().map(|p| p.to_string_lossy().to_string());

    // 3. Проверка iptables NAT редиректов (Linux) с точным поиском порта AdGuard / XKEEN_ADGUARD
    let iptables_redirect_active = is_adguard_iptables_active(cfg.dns_port);
    let dnsmasq_redirect_active = iptables_redirect_active;

    // 4. Проверка риска циклов
    let mut loop_risk = false;
    if status.running {
        if let Ok(dns_info) = agh_request(http, cfg, reqwest::Method::GET, "/control/dns_info", None, 3).await {
            let mut upstreams = Vec::new();
            if let Some(arr) = dns_info.get("upstream_dns").and_then(|a| a.as_array()) {
                for item in arr {
                    if let Some(s) = item.as_str() {
                        upstreams.push(s.to_string());
                    }
                }
            }
            let check_port = if cfg.dns_port > 0 { cfg.dns_port } else { DEFAULT_ADGUARD_PORT };
            let (lr, lw) = detect_agh_loop_risk(&upstreams, router_ip, check_port);
            loop_risk = lr;
            if let Some(w) = lw {
                recommendations.push(w);
            }
        }
    }

    // Рекомендации
    if !status.running {
        if detected_service_path.is_none() && detected_binary_path.is_none() {
            recommendations.push("AdGuard Home не установлен в системе Entware. Нажмите «Установить и запустить AdGuard Home» или выполните 'opkg install adguardhome'.".to_string());
        } else if detected_service_path.is_none() && detected_binary_path.is_some() {
            recommendations.push("Бинарный файл AdGuard Home обнаружен. Init-скрипт будет автоматически создан при первом запуске службы.".to_string());
        } else {
            recommendations.push("Служба AdGuard Home остановлена или веб-интерфейс недоступен на порту 3000.".to_string());
        }
    } else if !port_53_active && cfg.dns_port == 53 {
        recommendations.push("AdGuard Home запущен, но UDP-порт DNS 53 не отвечает на запросы.".to_string());
    }

    if status.running && cfg.dns_port != 53 && !iptables_redirect_active {
        recommendations.push(format!(
            "Причина 0 заблокированной рекламы: AdGuard Home слушает порт {} (порт 53 занят Keenetic ndnproxy). Запросы устройств идут на 53 порт и не попадают в фильтр AdGuard. Настройте upstream в Keenetic ndnproxy на 127.0.0.1:{} или включите перенаправление DNS в iptables.",
            cfg.dns_port, cfg.dns_port
        ));
    }

    if status.running && !status.protection_enabled {
        recommendations.push("Защита AdGuard Home временно выключена — блокировка рекламы и трекеров не выполняется.".to_string());
    }

    AghDiagnostics {
        port_53_status,
        port_3000_status,
        dnsmasq_redirect_active,
        iptables_redirect_active,
        detected_service_path,
        detected_config_path,
        loop_risk,
        recommendations,
    }
}

/// Поиск существующего init-скрипта AdGuard Home в Entware
pub fn find_adguard_init_script() -> Option<PathBuf> {
    let standard = [
        "/opt/etc/init.d/S99adguardhome",
        "/opt/etc/init.d/S99AdGuardHome",
        "/opt/etc/init.d/S99adguard",
        "/opt/etc/init.d/S99AdguardHome",
        "/opt/etc/init.d/adguardhome",
    ];
    for p in &standard {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Some(path);
        }
    }

    if let Ok(entries) = std::fs::read_dir("/opt/etc/init.d") {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                let name = path.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
                if name.contains("adguard") {
                    return Some(path);
                }
            }
        }
    }
    None
}

/// Поиск исполняемого файла AdGuardHome в стандартных путях Keenetic/Entware и PATH
pub fn find_adguard_binary() -> Option<PathBuf> {
    let standard = [
        "/opt/bin/AdGuardHome",
        "/opt/bin/adguardhome",
        "/opt/sbin/AdGuardHome",
        "/opt/sbin/adguardhome",
        "/opt/etc/AdGuardHome/AdGuardHome",
        "/opt/AdGuardHome/AdGuardHome",
        "/opt/root/AdGuardHome/AdGuardHome",
        "/opt/home/AdGuardHome/AdGuardHome",
        "/opt/var/AdGuardHome/AdGuardHome",
    ];
    for p in &standard {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Some(path);
        }
    }

    #[cfg(target_os = "linux")]
    {
        for cmd in &["AdGuardHome", "adguardhome"] {
            if let Ok(out) = std::process::Command::new("which").arg(cmd).output() {
                if out.status.success() {
                    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    if !s.is_empty() && std::path::Path::new(&s).is_file() {
                        return Some(PathBuf::from(s));
                    }
                }
            }
        }
    }

    None
}

/// Поиск конфигурационного файла AdGuardHome.yaml

/// Генерация автономного скрипта инициализации /opt/etc/init.d/S99adguardhome
pub fn generate_adguard_init_script_content(bin_path: &str, conf_path: &str, work_dir: &str) -> String {
    format!(
r#"#!/bin/sh

export PATH="/opt/sbin:/opt/bin:/usr/sbin:/usr/bin:/sbin:/bin"

NAME="AdGuardHome"
BIN="{bin_path}"
WORK_DIR="{work_dir}"
CONF="{conf_path}"
PID_FILE="/opt/var/run/AdGuardHome.pid"
LOG_FILE="/opt/var/log/adguardhome.log"

mkdir -p /opt/var/run /opt/var/log "$WORK_DIR" 2>/dev/null

is_running() {{
    if [ -f "$PID_FILE" ]; then
        pid=$(cat "$PID_FILE" 2>/dev/null)
        if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
            return 0
        fi
    fi
    pid=$(pidof AdGuardHome 2>/dev/null || pgrep -f "$BIN" 2>/dev/null)
    if [ -n "$pid" ]; then
        echo "$pid" | awk '{{print $1}}' > "$PID_FILE"
        return 0
    fi
    return 1
}}

start() {{
    if is_running; then
        echo "$NAME is already running"
        return 0
    fi
    echo "Starting $NAME..."
    if [ ! -f "$CONF" ]; then
        mkdir -p "$(dirname "$CONF")" 2>/dev/null
    fi
    "$BIN" -c "$CONF" -w "$WORK_DIR" --no-check-update >> "$LOG_FILE" 2>&1 &
    pid=$!
    echo "$pid" > "$PID_FILE"
    sleep 1
    if kill -0 "$pid" 2>/dev/null; then
        echo "$NAME started (pid $pid)"
        return 0
    else
        echo "Failed to start $NAME, check $LOG_FILE" >&2
        return 1
    fi
}}

stop() {{
    echo "Stopping $NAME..."
    if is_running; then
        pid=$(cat "$PID_FILE" 2>/dev/null)
        [ -n "$pid" ] && kill "$pid" 2>/dev/null
        sleep 1
        killall -15 AdGuardHome 2>/dev/null || true
        sleep 1
        killall -9 AdGuardHome 2>/dev/null || true
        rm -f "$PID_FILE"
    else
        killall -9 AdGuardHome 2>/dev/null || true
    fi
    echo "$NAME stopped"
    return 0
}}

restart() {{
    stop
    sleep 1
    start
}}

status() {{
    if is_running; then
        echo "$NAME is running (pid $(cat "$PID_FILE"))"
        return 0
    else
        echo "$NAME is stopped"
        return 1
    fi
}}

case "$1" in
    start) start ;;
    stop) stop ;;
    restart) restart ;;
    status) status ;;
    *) echo "Usage: $0 {{start|stop|restart|status}}" >&2; exit 1 ;;
esac
"#
    )
}

/// Определение архитектуры AdGuard Home по строке архитектуры (из uname или cfg)
pub fn detect_adguard_arch_from_str(arch_str: &str) -> &'static str {
    let m = arch_str.trim().to_lowercase();
    if m.contains("aarch64") || m.contains("arm64") || m.contains("armv8") {
        "arm64"
    } else if m.contains("armv7") || m.contains("armhf") {
        "armv7"
    } else if m.contains("armv6") {
        "armv6"
    } else if m.contains("armv5") {
        "armv5"
    } else if m.contains("mipsle") || m.contains("mipsel") {
        "mipsle_softfloat"
    } else if m.contains("mips") {
        "mips_softfloat"
    } else if m.contains("x86_64") || m.contains("amd64") {
        "amd64"
    } else if m.contains("i386") || m.contains("i686") || m.contains("386") {
        "386"
    } else if m.contains("arm") {
        "armv7"
    } else {
        "arm64"
    }
}

/// Определение архитектуры для официальных архивов AdGuard Home
pub fn detect_adguard_arch() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        if let Ok(out) = std::process::Command::new("uname").arg("-m").output() {
            let m = String::from_utf8_lossy(&out.stdout);
            return detect_adguard_arch_from_str(&m);
        }
    }

    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else if cfg!(target_arch = "arm") {
        "armv7"
    } else if cfg!(target_arch = "mips") {
        if cfg!(target_endian = "little") {
            "mipsle_softfloat"
        } else {
            "mips_softfloat"
        }
    } else if cfg!(target_arch = "x86_64") {
        "amd64"
    } else {
        "armv7"
    }
}

/// Генерация готовой стандартной конфигурации AdGuardHome.yaml
/// Задает schema_version: 29 для полного пропуска мастера первичной настройки (setup wizard)

/// Проверка наличия или автоматическое создание init-скрипта службы
pub fn ensure_adguard_init_script() -> Result<PathBuf, String> {
    if let Some(script) = find_adguard_init_script() {
        return Ok(script);
    }

    let bin = find_adguard_binary().ok_or_else(|| {
        "Бинарный файл AdGuardHome не найден в системе (проверены /opt/AdGuardHome, /opt/bin, /opt/sbin и PATH)".to_string()
    })?;

    let conf = find_adguard_config().unwrap_or_else(|| {
        let is_53_free = std::net::UdpSocket::bind("0.0.0.0:53").is_ok();
        let port = if is_53_free { 53 } else { DEFAULT_ADGUARD_DNS_FALLBACK_PORT };
        ensure_default_adguard_config(port, 3000)
    });
    let work_dir = conf
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "/opt/etc/AdGuardHome".to_string());

    let target_script = PathBuf::from("/opt/etc/init.d/S99adguardhome");
    if let Some(parent) = target_script.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let content = generate_adguard_init_script_content(
        &bin.to_string_lossy(),
        &conf.to_string_lossy(),
        &work_dir,
    );

    std::fs::write(&target_script, content)
        .map_err(|e| format!("Не удалось создать init-скрипт {:?}: {e}", target_script))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&target_script, std::fs::Permissions::from_mode(0o755));
    }

    crate::log_i!("Автоматически создан init-скрипт службы AdGuard Home: {:?}", target_script);
    Ok(target_script)
}

/// Автоматическая загрузка официального бинарника AdGuard Home с CDN и установка
pub async fn install_adguard_package() -> Result<String, String> {
    #[cfg(target_os = "linux")]
    {
        let arch = detect_adguard_arch();
        crate::log_i!("Запуск загрузки и установки AdGuard Home для архитектуры '{}'...", arch);

        let log_file = "/tmp/agh_install.log";
        let script_cmd = format!(
r#"
ARCH="{arch}"
URL1="https://static.adguard.com/adguardhome/release/AdGuardHome_linux_${{ARCH}}.tar.gz"
URL2="https://github.com/AdguardTeam/AdGuardHome/releases/latest/download/AdGuardHome_linux_${{ARCH}}.tar.gz"

TMP_DIR="/opt/tmp"
mkdir -p "$TMP_DIR" /opt/var/log /opt/var/run /opt/etc/AdGuardHome /opt/bin 2>/dev/null
TAR_FILE="$TMP_DIR/AdGuardHome.tar.gz"
rm -f "$TAR_FILE"

download_file() {{
    dl_url="$1"
    echo "Загрузка с $dl_url ..."
    if command -v curl >/dev/null 2>&1; then
        if curl -f -s -S -L --connect-timeout 15 --max-time 180 "$dl_url" -o "$TAR_FILE"; then
            return 0
        fi
    fi
    if command -v wget >/dev/null 2>&1; then
        if wget -q -T 30 -O "$TAR_FILE" "$dl_url"; then
            return 0
        fi
    fi
    return 1
}}

SUCCESS=0
for u in "$URL1" "$URL2"; do
    if download_file "$u"; then
        if [ -s "$TAR_FILE" ]; then
            FILE_SIZE=$(wc -c < "$TAR_FILE" 2>/dev/null || echo 0)
            if [ "$FILE_SIZE" -gt 1000000 ]; then
                SUCCESS=1
                echo "Архив успешно скачан ($FILE_SIZE байт)"
                break
            fi
        fi
    fi
    rm -f "$TAR_FILE"
done

# Резервная попытка через opkg (если пакет вдруг доступен в кастомном репозитории)
if [ "$SUCCESS" -ne 1 ]; then
    echo "Прямая загрузка с CDN не удалась, проверка opkg..."
    if command -v opkg >/dev/null 2>&1; then
        opkg update >/dev/null 2>&1 && opkg install adguardhome && SUCCESS=2
    fi
fi

if [ "$SUCCESS" -eq 0 ]; then
    echo "Ошибка: не удалось скачать архив AdGuardHome_linux_${{ARCH}}.tar.gz с CDN static.adguard.com и GitHub." >&2
    exit 1
fi

if [ "$SUCCESS" -eq 1 ]; then
    echo "Распаковка $TAR_FILE в /opt..."
    tar -xzf "$TAR_FILE" -C /opt
    rm -f "$TAR_FILE"
fi

if [ -f /opt/AdGuardHome/AdGuardHome ]; then
    chmod 755 /opt/AdGuardHome/AdGuardHome
    ln -sf /opt/AdGuardHome/AdGuardHome /opt/bin/AdGuardHome
elif [ -f /opt/bin/AdGuardHome ]; then
    chmod 755 /opt/bin/AdGuardHome
elif [ -f /opt/sbin/AdGuardHome ]; then
    chmod 755 /opt/sbin/AdGuardHome
else
    echo "Ошибка: бинарный файл AdGuardHome не найден после распаковки архива." >&2
    exit 1
fi

echo "AdGuard Home успешно установлен."
"#
        );

        let timeout_res = tokio::time::timeout(
            Duration::from_secs(210),
            tokio::process::Command::new("sh")
                .arg("-c")
                .arg(format!("exec sh -c '{script_cmd}' </dev/null >{log_file} 2>&1"))
                .status(),
        )
        .await;

        let status = match timeout_res {
            Ok(Ok(st)) => st,
            Ok(Err(e)) => return Err(format!("Ошибка запуска процесса установки: {e}")),
            Err(_) => return Err("Превышен таймаут загрузки и установки AdGuard Home (210 сек)".to_string()),
        };

        let log = tokio::fs::read_to_string(log_file).await.unwrap_or_default().trim().to_string();

        if !status.success() {
            return Err(format!(
                "Ошибка скачивания или установки AdGuard Home (код {:?}): {}",
                status.code(),
                if log.is_empty() { "нет данных в логе" } else { &log }
            ));
        }

        let bin = find_adguard_binary().ok_or_else(|| {
            format!("Установка завершена, но бинарный файл не обнаружен в /opt/AdGuardHome/AdGuardHome. Лог: {log}")
        })?;

        // Создаем готовый конфиг AdGuardHome.yaml (если его еще нет), чтобы не требовалось проходить визард
        let is_53_free = std::net::UdpSocket::bind("0.0.0.0:53").is_ok();
        let dns_port = if is_53_free { 53 } else { 5353 };
        let conf_path = ensure_default_adguard_config(dns_port, 3000);

        // Создаем init-скрипт службы
        let script = ensure_adguard_init_script().map_err(|e| {
            format!("Бинарник установлен ({:?}), но не удалось создать init-скрипт: {e}", bin)
        })?;

        crate::log_i!("AdGuard Home успешно установлен: bin={:?}, conf={:?}, script={:?}", bin, conf_path, script);

        Ok(format!(
            "AdGuard Home ({arch}) успешно загружен с официального CDN, распакован и настроен (DNS порт: {dns_port}, Web UI порт: 3000)."
        ))
    }

    #[cfg(not(target_os = "linux"))]
    {
        Ok("[dev/win] Пакет AdGuard Home успешно установлен (эмуляция)".to_string())
    }
}

/// Переключить флаг ENABLED=yes/no в init-скрипте Entware для AdGuard Home.
#[cfg(target_os = "linux")]
fn set_init_script_enabled(script_path: &std::path::Path, enabled: bool) -> Result<(), String> {
    let raw = std::fs::read_to_string(script_path).map_err(|e| e.to_string())?;
    let target = if enabled { "ENABLED=yes" } else { "ENABLED=no" };
    let mut changed = false;
    let updated: Vec<String> = raw
        .lines()
        .map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("ENABLED=") {
                if trimmed != target {
                    changed = true;
                }
                target.to_string()
            } else {
                line.to_string()
            }
        })
        .collect();
    if changed {
        let mut content = updated.join("\n");
        if raw.ends_with('\n') {
            content.push('\n');
        }
        std::fs::write(script_path, content).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Управление системной службой AdGuard Home (/opt/etc/init.d/S99adguardhome)
pub async fn service_action(action: &str) -> Result<String, String> {
    let act = action.trim().to_lowercase();
    if !["start", "stop", "restart", "status"].contains(&act.as_str()) {
        return Err(format!("Недопустимое действие службы: '{action}'"));
    }

    #[cfg(target_os = "linux")]
    {
        if act == "stop" {
            if let Ok(script_path) = ensure_adguard_init_script() {
                let script = script_path.to_string_lossy().to_string();
                if crate::config::is_valid_init_script_path(&script) {
                    let cmd_str = format!("exec '{script}' 'stop' </dev/null >/tmp/agh_service.log 2>&1");
                    let _ = tokio::time::timeout(
                        Duration::from_secs(10),
                        tokio::process::Command::new("sh").arg("-c").arg(&cmd_str).status(),
                    )
                    .await;
                    let _ = set_init_script_enabled(&script_path, false);
                }
            }
            let _ = remove_adguard_iptables();
            let _ = tokio::process::Command::new("sh")
                .arg("-c")
                .arg("killall -9 AdGuardHome 2>/dev/null || true; pkill -9 -f AdGuardHome 2>/dev/null || true")
                .status()
                .await;
            return Ok("Служба AdGuard Home остановлена и выгружена из памяти".to_string());
        }

        let script_path = match ensure_adguard_init_script() {
            Ok(p) => p,
            Err(e) => {
                if act == "start" {
                    crate::log_i!("Служба AdGuard Home не найдена, выполняем автоматическую установку с CDN...");
                    match install_adguard_package().await {
                        Ok(_) => ensure_adguard_init_script().map_err(|e2| {
                            format!("AdGuard Home был установлен, но init-скрипт не найден: {e2}")
                        })?,
                        Err(install_err) => {
                            return Err(format!(
                                "AdGuard Home не установлен. Попытка автоматической загрузки завершилась с ошибкой: {install_err}"
                            ));
                        }
                    }
                } else {
                    return Err(e);
                }
            }
        };

        if act == "start" || act == "restart" {
            let _ = set_init_script_enabled(&script_path, true);
        }

        let script = script_path.to_string_lossy().to_string();
        if !crate::config::is_valid_init_script_path(&script) {
            return Err(format!("Недопустимый путь к init-скрипту AdGuard Home: '{script}'"));
        }
        let log_file = "/tmp/agh_service.log";

        // КРИТИЧЕСКИ ВАЖНО:
        // Перенаправление stdin/stdout/stderr (exec ... </dev/null >/tmp/agh_service.log 2>&1)
        // и жесткий таймаут 15 секунд исключают зависание tokio на пайпах фонового демона!
        let cmd_str = format!("exec '{script}' '{act}' </dev/null >'{log_file}' 2>&1");
        let timeout_res = tokio::time::timeout(
            Duration::from_secs(15),
            tokio::process::Command::new("sh")
                .arg("-c")
                .arg(&cmd_str)
                .status(),
        )
        .await;

        let status = match timeout_res {
            Ok(Ok(st)) => st,
            Ok(Err(e)) => return Err(format!("Ошибка запуска команды для {script}: {e}")),
            Err(_) => return Err(format!("Таймаут (15 сек) выполнения службы {script} {act}")),
        };

        let log_content = tokio::fs::read_to_string(log_file)
            .await
            .unwrap_or_default()
            .trim()
            .to_string();

        if act == "status" {
            let is_running = crate::system::is_process_running("AdGuardHome");
            return Ok(if is_running {
                "Служба AdGuard Home активна (работает)".to_string()
            } else {
                "Служба AdGuard Home остановлена".to_string()
            });
        }

        // Для start и restart: верифицируем фактический запуск процесса
        tokio::time::sleep(Duration::from_millis(800)).await;
        let mut is_running = crate::system::is_process_running("AdGuardHome");

        // Если процесс не запустился из-за конфликта порта 53 (ndnproxy на Keenetic) или 5353 (avahi-daemon), автоматически переключаем на 5354
        if !is_running && (act == "start" || act == "restart") {
            if let Ok(agh_log) = tokio::fs::read_to_string("/opt/var/log/adguardhome.log").await {
                if agh_log.contains("address already in use") || agh_log.contains("bind: address already in use") {
                    crate::log_w!("Обнаружен конфликт порта DNS. Автоматическое переключение DNS-порта AdGuard Home на {}...", DEFAULT_ADGUARD_DNS_FALLBACK_PORT);
                    if let Some(conf_path) = find_adguard_config() {
                        if let Ok(conf_str) = tokio::fs::read_to_string(&conf_path).await {
                            let (patched, _) = sanitize_and_heal_adguard_yaml(&conf_str, DEFAULT_ADGUARD_DNS_FALLBACK_PORT);
                            let _ = crate::api::atomic_write_file(&conf_path, &patched).await;
                            let cmd_retry = format!("exec '{script}' '{act}' </dev/null >'{log_file}' 2>&1");
                            let _ = tokio::process::Command::new("sh").arg("-c").arg(&cmd_retry).status().await;
                            tokio::time::sleep(Duration::from_millis(1000)).await;
                            is_running = crate::system::is_process_running("AdGuardHome");
                        }
                    }
                }
            }
        }

        if is_running {
            if act == "start" || act == "restart" {
                let active_port = detect_adguard_dns_port().unwrap_or(DEFAULT_ADGUARD_DNS_FALLBACK_PORT);
                let _ = apply_adguard_iptables(active_port);
            }
            let msg = if !log_content.is_empty() {
                format!("Служба AdGuard Home успешно выполнила команду '{act}': {log_content}")
            } else {
                format!("Служба AdGuard Home успешно выполнила команду '{act}'")
            };
            Ok(msg)
        } else {
            let mut err_msg = format!("Команда '{act}' завершилась (код {:?}), но процесс AdGuardHome не обнаружен.", status.code());
            if !log_content.is_empty() {
                err_msg.push_str(&format!(" Вывод службы: {log_content}."));
            }
            if let Ok(agh_log) = tokio::fs::read_to_string("/opt/var/log/adguardhome.log").await {
                let tail = agh_log.lines().rev().take(5).collect::<Vec<_>>();
                if !tail.is_empty() {
                    let reversed_tail = tail.into_iter().rev().collect::<Vec<_>>().join(" | ");
                    if reversed_tail.contains("address already in use") || reversed_tail.contains("bind:") {
                        err_msg.push_str(" Внимание: Порт DNS (53) или веб-панели (3000) занят другой службой роутера (например, dnsmasq).");
                    }
                    err_msg.push_str(&format!(" Лог: {reversed_tail}"));
                }
            }
            Err(err_msg)
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        Ok(format!("[dev/win] Выполнена команда службы AdGuard Home: {act}"))
    }
}

/// Переключение AdBlock независимо от интеграции AdGuard в Mihomo (баг 116).
pub fn apply_adblock_toggle(cfg: &mut crate::config::AppConfig, target: bool) {
    cfg.adblock_enabled = target;
}

/// Переключение интеграции AdGuard в Mihomo независимо от AdBlock (баг 116).
/// Фиксирует выбор пользователя, чтобы автоподхват сторожа больше не перезаписывал настройку.
pub fn apply_integration_toggle(cfg: &mut crate::config::AppConfig, target: bool) {
    cfg.adguard.enabled = target;
    cfg.adguard.integration_choice_locked = true;
}

/// Шаг переключения AdBlock (баг 60): общий план для API и Telegram-бота.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdblockStep {
    CommitConfig,
    SetProtection(bool),
    SyncIptables { enable: bool, port: u16 },
    ResyncTunnels,
}

/// Чистый планировщик переключения AdBlock (баг 60):
/// CommitConfig всегда строго первый, и только потом действия над AdGuard, iptables и туннелями.
pub fn plan_adblock_toggle(cfg: &crate::config::AppConfig, target: bool) -> Vec<AdblockStep> {
    vec![
        AdblockStep::CommitConfig,
        AdblockStep::SetProtection(target),
        AdblockStep::SyncIptables {
            enable: target,
            port: cfg.adguard.dns_port,
        },
        AdblockStep::ResyncTunnels,
    ]
}

