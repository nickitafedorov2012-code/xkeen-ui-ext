//! VK-туннель: обход белых списков через TURN-серверы ВК-звонков на ядре csqtt
//! (клиент из Afteroid/csqttr «CSQTT-Rebirth», PolyForm Noncommercial 1.0.0, © amurcanov;
//! сборка для роутера — `scripts/build-csqtt-core.sh`).
//! Панель сама скачивает клиент, создаёт для него TUN-интерфейс, запускает и стережёт процесс
//! и подключает туннель к Mihomo как сервер второго уровня (Tier 2, «БС»).
//!
//! Что проверено по исходникам csqttr (коммит CORE_UPSTREAM_COMMIT, 05.11.2026):
//! - флаги клиента: `-peer host:port -vk <хеши через запятую> -n <потоки, кратно 10> -listen 127.0.0.1:0
//!   -tun-uds <имя> -captcha-mode auto`; пароль — только в переменной окружения `CSQTT_PASSWORD`
//!   (в списке процессов его не видно);
//! - при `CSQTT_EVENTS=1` клиент печатает в stdout машинные события `__CSQTT_EVENT__|<ТИП>|<json>`:
//!   READY, STATS (active, bytes_up, bytes_down), CONFIG (`TUNCONF:<ip>:<dns>:…` — адрес туннеля от сервера),
//!   ERROR (code, message, fatal), TUN_LOST;
//! - трафик идёт через TUN: клиент слушает абстрактный unix-сокет `-tun-uds` и ждёт по нему дескриптор TUN
//!   (SCM_RIGHTS, байт 1 в ответ). На Android его создаёт VpnService, на роутере — панель (интерфейс `csq0`);
//! - Mihomo выводит в туннель трафик сервера «📞 VK-туннель [БС]» (`type: direct`, `interface-name: csq0`);
//! - ручная капча: клиент печатает `CAPTCHA_SOLVE|<mode>|<redirect_uri>|<session_token>` и ждёт
//!   `CAPTCHA_RESULT|<token>` в stdin. На роутере WebView нет — панель сразу отвечает ошибкой,
//!   и клиент продолжает встроенное автоматическое решение без ожидания таймаутов; `STOP` — мягкая остановка.

use std::collections::VecDeque;

use serde::Serialize;
use tokio::sync::RwLock;

use crate::config::VkTurnConfig;

/// Тег релиза xkeen-ui-ext, в котором опубликованы бинарники ядра (см. scripts/build-csqtt-core.sh).
pub const CORE_TAG: &str = "csqtt-core-mw-0e90df2";
/// Репозиторий и коммит, из которых собрано ядро.
pub const CORE_UPSTREAM: &str = "Afteroid/csqttr";
pub const CORE_UPSTREAM_COMMIT: &str = "0e90df237583738741b50e86e4f51f87e96c94da";
pub const CORE_RELEASES: &str = "https://github.com/nickitafedorov2012-code/xkeen-ui-ext/releases/download";

// OAuth параметры официального приложения VK для звонков (CSQTT)
pub const VK_APP_ID: &str = "7793118";
pub const VK_OAUTH_SCOPE: &str = "1073737727";
pub const VK_REDIRECT_URI: &str = "https://oauth.vk.ru/blank.html";
pub const VK_AUTH_URL: &str = "https://oauth.vk.ru/authorize?client_id=7793118&scope=1073737727&redirect_uri=https%3A%2F%2Foauth.vk.ru%2Fblank.html&display=page&response_type=token&revoke=1&v=5.199";

pub const CORE_PATH: &str = "/opt/sbin/csqtt-client";
/// Ядро прошлой версии (WDTT): удаляется при установке нового, чтобы не занимать место.
pub const LEGACY_CORE_PATH: &str = "/opt/sbin/wdtt-client";
pub const WORK_DIR: &str = "/opt/var/lib/csqtt";
pub const PID_FILE: &str = "/opt/var/run/csqtt-client.pid";
pub const PROVIDER_ID: &str = "vkturn";
pub const PROXY_NAME: &str = "📞 VK-туннель [БС]";
/// PEER-порт сервера csqtt на VPS по умолчанию (deploy.sh: `CSQTT_PEER_PORT`, 46000).
pub const DEFAULT_SERVER_PORT: u16 = 46000;
/// TUN-интерфейс, через который Mihomo отправляет трафик в туннель.
pub const TUN_IFACE: &str = "csq0";
/// Имя абстрактного unix-сокета, по которому клиент ждёт дескриптор TUN.
pub const TUN_UDS_NAME: &str = "xkeen_route_csqtt_tun";
/// MTU для TUN-интерфейса поверх WebRTC/TURN/LTE: с запасом ~140 байт на заголовки csqtt + WRAP-SRTP + TURN + UDP/IP
/// внешний пакет укладывается в ~1040 байт и не дропается на мобильных LTE-каналах с пониженным Path MTU.
pub const TUN_MTU: u16 = 900;
/// Максимальный TCP MSS для соединений через csq0 (900 - 60 байт запаса на IP/TCP заголовки и опции).
pub const TUN_TCP_MSS: u16 = 840;
/// MTU и TCP MSS для режима `-turn-transport tcp`.
/// Инвариант ядра csqttr (`if packet.len() <= 1280`, коммит 0e90df23758):
/// `MSS + 52 (IPv4 + TCP с timestamps) <= 1280` (1220 + 52 = 1272 <= 1280) и `MSS + 60 <= MTU`,
/// чтобы пакеты не резались фрагментатором `wire_protocol::fragment_packet` 1 -> 2.
pub const TUN_MTU_TCP: u16 = 1280;
pub const TUN_TCP_MSS_TCP: u16 = 1220;
/// Устаревшее значение TCP MSS (1300) из прежних сборок: подчищаем в iptables mangle,
/// чтобы после обновления панели без перезагрузки роутера не оставалось висячих правил.
pub(super) const LEGACY_TUN_TCP_MSS_TCP: u16 = 1300;
/// Верхний предел потоков при авто-переключении на TCP (снижает нагрузку на CPU роутера).
pub const TCP_AUTO_WORKERS_CAP: u32 = 10;
/// Отдельная таблица маршрутизации для сокетов, привязанных к csq0 (в пределах 1..252 для Busybox).
pub const ROUTE_TABLE: u32 = 222;
/// Подсети инфраструктуры ВК (AS47541) и TURN-серверов звонков. Трафик клиента туда идёт строго напрямую.
/// Конкретные адреса TURN, которые ВК выдал клиенту, панель дополнительно берёт из его лога.
pub const VK_NETWORK_CIDRS: &[&str] = &[
    "155.212.192.0/20",
    "87.240.128.0/18",
    "93.186.224.0/20",
    "95.142.192.0/20",
    "95.213.0.0/17",
    "185.32.248.0/22",
];
pub const VK_TURN_CIDRS: &[&str] = &["155.212.192.0/20"];

/// Хосты VK API из focsq (schedule_vk_endpoint_excludes), которые ядро посещает напрямую
pub const VK_API_HOSTS: &[&str] = &[
    "api.vk.me",
    "api.vk.com",
    "api.vk.ru",
    "login.vk.ru",
    "login.vk.com",
    "id.vk.ru",
    "id.vk.com",
    "oauth.vk.ru",
    "oauth.vk.com",
];
pub const RULES_BEGIN: &str = "# BEGIN xkeen-route vkturn";
pub const RULES_END: &str = "# END xkeen-route vkturn";
const LOG_TAIL: usize = 300;
/// csqtt делит потоки на группы по 10 (`WORKERS_PER_GROUP` в worker.rs), каждая группа берёт свой хеш звонка.
pub const WORKERS_PER_GROUP: u32 = 10;
/// Автоматическое значение для роутера. Ручной выбор по-прежнему поддерживает до 100 потоков.
pub const DEFAULT_WORKERS: u32 = 20;
/// `MAX_WORKERS` в main.rs клиента.
pub const MAX_WORKERS: u32 = 100;
/// Максимум ссылок: все используются одновременно (`MAX_VK_HASHES` в main.rs клиента).
pub const MAX_LINKS: usize = 6;
/// Сколько адресов TURN запоминаем для исключений (защита от разрастания правил).
const MAX_TURN_IPS: usize = 32;
const EVENT_PREFIX: &str = "__CSQTT_EVENT__|";
/// Диагностика этапов каждого потока клиента csqtt (stream_diagnostics.rs).
const STREAM_PREFIX: &str = "__CSQTT_STREAM__|";
/// Сколько тайм-аутов GETCONF подряд необходимо для сообщения о недоступности сервера.
const GETCONF_SILENT_THRESHOLD: u32 = 3;
const SERVER_SILENT_ON_PASSWORD: &str =
    "Сервер csqtt не ответил на запрос конфигурации: проверьте соединение и пароль подключения";
const CAPTCHA_SOLVE_PREFIX: &str = "CAPTCHA_SOLVE|";
/// Ответ клиенту на запрос WebView-капчи: на роутере WebView нет.
pub const CAPTCHA_NO_WEBVIEW_REPLY: &str = "CAPTCHA_RESULT|error:webview unavailable on router\n";

/// SHA-256 бинарников CORE_TAG (вывод scripts/build-csqtt-core.sh).
pub const CORE_SHA256: &[(&str, &str)] = &[
    ("csqtt-client-linux-arm64", "835410a60aa2c413030260ba80e3efa8255f02326395108321a63c2597a1bef7"),
];

/// Сопоставление с архитектурами от updater::detect_mihomo_arch().
/// Клиент csqtt написан на Rust; для MIPS-роутеров у Rust нет стабильной сборки с musl.
pub fn core_asset_for_arch(arch: &str) -> Option<&'static str> {
    let a = arch.to_ascii_lowercase();
    if a.contains("arm64") || a.contains("aarch64") {
        Some("csqtt-client-linux-arm64")
    } else {
        None
    }
}

/// Нормализация настройки транспорта TURN: "auto" | "udp" | "tcp".
pub fn normalize_turn_transport(raw: &str) -> Result<&'static str, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "" | "auto" => Ok("auto"),
        "udp" => Ok("udp"),
        "tcp" => Ok("tcp"),
        other => Err(format!("Некорректный транспорт TURN: {other} (ожидается auto, udp или tcp)")),
    }
}

/// Число потоков с учётом транспорта: для TCP не нужно 30 потоков, ограничиваем до TCP_AUTO_WORKERS_CAP при автовыборе.
pub fn effective_workers_for_transport(cfg: &VkTurnConfig, transport: &str) -> u32 {
    let base = effective_workers(cfg);
    if transport == "tcp" && cfg.streams == 0 {
        base.min(TCP_AUTO_WORKERS_CAP)
    } else {
        base
    }
}

// ---------- Ссылки на звонки, адрес сервера, пароль ----------

/// Строгая проверка ссылки (или голого хеша) на звонок ВК. Возвращает хеш звонка.
/// Хеш уходит аргументом процесса и в config.json — только безопасные символы.

pub mod auth;
pub mod health;
pub mod tunnel;

pub use auth::*;
pub use health::*;
pub use tunnel::*;

#[cfg(test)]
mod tests;
