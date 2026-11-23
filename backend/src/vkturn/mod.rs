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
#[cfg(test)]
use std::collections::BTreeSet;
#[cfg(test)]
use std::sync::Arc;
#[cfg(test)]
use std::time::{Duration, Instant};

use serde::Serialize;
#[cfg(test)]
use tokio::io::AsyncWriteExt;
use tokio::sync::RwLock;
#[cfg(test)]
use tokio::sync::mpsc;

use crate::config::VkTurnConfig;

/// Тег релиза xkeen-ui-ext, в котором опубликованы бинарники ядра (см. scripts/build-csqtt-core.sh).
pub const CORE_TAG: &str = "csqtt-core-0e90df2";
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
    ("csqtt-client-linux-arm64", "27cbeab0783758820c3973c7eda81b7ca83bcaed84b5579be5c0521509f9053b"),
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
mod tests {
    use super::*;

    fn cfg_with(links: &[&str]) -> VkTurnConfig {
        let mut cfg = VkTurnConfig::default();
        cfg.enabled = true;
        cfg.peer = "1.2.3.4:46000".into();
        cfg.password = "s3cret-pass".into();
        cfg.links = links.iter().map(|s| s.to_string()).collect();
        cfg
    }

    // Регрессия: установка ядра шла внутри запроса и обрывалась таймаутом веб-панели (30 с).
    // Теперь она фоновая: повторное нажатие не запускает вторую, итог виден в статусе.
    #[test]
    fn core_install_runs_once_and_reports_result() {
        let mgr = VkTurnManager::with_core_path("/nonexistent/csqtt-client");
        assert_eq!(mgr.install_state(), CoreInstall::default());
        assert!(mgr.begin_install());
        assert!(!mgr.begin_install(), "вторая установка не должна стартовать, пока идёт первая");
        mgr.install.lock().unwrap().downloaded = 4096;
        assert!(mgr.install_state().running);
        mgr.finish_install(&Err("данные перестали приходить".into()));
        let st = mgr.install_state();
        assert!(!st.running);
        assert_eq!(st.error.as_deref(), Some("данные перестали приходить"));
        // Новая попытка сбрасывает прошлую ошибку и прогресс
        assert!(mgr.begin_install());
        assert_eq!(mgr.install_state(), CoreInstall { running: true, ..Default::default() });
        mgr.finish_install(&Ok("ok".into()));
        assert_eq!(mgr.install_state().error, None);
    }

    #[test]
    fn test_parse_call_link() {
        assert_eq!(parse_call_link("https://vk.com/call/join/abc12345").unwrap(), "abc12345");
        assert_eq!(
            parse_call_link("  https://vk.com/call/join/Very_Long-Valid_Hash12345/  ").unwrap(),
            "Very_Long-Valid_Hash12345"
        );
        assert_eq!(parse_call_link("https://vk.ru/call/join/abc12345?from=app").unwrap(), "abc12345");
        assert_eq!(parse_call_link("abcDEF_12-34").unwrap(), "abcDEF_12-34", "голый хеш тоже принимается");
        // Реальная ссылка пользователя (vk.ru, хеш с _ и -)
        assert_eq!(
            parse_call_link("https://vk.ru/call/join/dVi6izW61Ty9UuE1hEkU_uSdoNuihCgPQnILjgk4AdI").unwrap(),
            "dVi6izW61Ty9UuE1hEkU_uSdoNuihCgPQnILjgk4AdI"
        );

        let e = parse_call_link("https://telemost.yandex.ru/j/12345678").unwrap_err();
        assert!(e.contains("Телемост"));

        assert!(parse_call_link("http://vk.com/call/join/abc12345").is_err());
        assert!(parse_call_link("https://vk.com.evil.ru/call/join/abc12345").is_err());
        assert!(parse_call_link("https://other.domain.com/call/join/abc12345").is_err());
        assert!(parse_call_link("https://vk.com/call/join/abc12;reboot").is_err());
        assert!(parse_call_link("https://vk.com/call/join/abc$(id)").is_err());
        assert!(parse_call_link("https://vk.com/call/join/short").is_err());
        assert!(parse_call_link("https://vk.com/call/join/has space 1234").is_err());
        assert!(parse_call_link("https://vk.com/call/join/has\nnewline123").is_err());
        assert!(parse_call_link("abc,def12345").is_err(), "запятая сломала бы список -vk");
    }

    #[test]
    fn regression_android_encrypted_profile_is_readable() {
        let link = "csqtt://rGMTblReroqI7mbb2bHPcn2Ht7npV8ThxgOFgtHXHPhRHTbmWrXbOg8IO7oBJMO7wbpulj877bUgDF3do4Af6y1VvT9ipPKD4wwp7_lE6mItWw";
        let (peer, password) = parse_csqtt_link(link).expect("encrypted Android profile");
        assert_eq!(peer, "example.com:46000");
        assert_eq!(password, "dummy");
        let mut corrupted = link.to_string();
        corrupted.pop();
        corrupted.push('A');
        assert!(parse_csqtt_link(&corrupted).is_none());
    }

    #[test]
    fn regression_csqtt_backend_preserves_encoded_password_and_rejects_lost_fields() {
        let parsed = parse_csqtt_link("csqtt://connect?v=2&host=example.com&peer=46000&password=abc%2Bxyz").unwrap();
        assert_eq!(parsed.1, "abc+xyz");
        assert!(parse_csqtt_link("csqtt://connect?v=2&host=example.com&peer=46000&password=p&token=profile").is_none());
        assert!(parse_csqtt_link("csqtt://connect?v=3&host=example.com&peer=46000&password=p").is_none());
        assert!(parse_csqtt_link("csqtt://connect?v=2&host=example.com&host=other.example&peer=46000&password=p").is_none());
    }

    #[test]
    fn regression_csqtt_backend_rejects_invalid_profile_ports() {
        assert!(parse_csqtt_link("csqtt://connect?v=2&host=example.com&peer=46000oops&password=p").is_none());
        assert!(parse_csqtt_link("csqtt://connect?v=2&host=example.com&peer=0&password=p").is_none());
        assert!(parse_csqtt_link("csqtt://p@example.com:99999").is_none());
        assert!(parse_csqtt_link("csqtt://p@example.com:46000oops").is_none());
    }

    #[test]
    fn regression_csqtt_backend_glued_query_and_scheme_less_call_links() {
        assert_eq!(
            parse_call_link("vk.com/call/join/abcDEF_1234567890").unwrap(),
            "abcDEF_1234567890"
        );
        assert_eq!(
            parse_call_link("m.vk.ru/call/join/abcDEF_1234567890?from=app").unwrap(),
            "abcDEF_1234567890"
        );
        let glued = "csqtt://connect?v=2host=example.com&peer=46000&password=secret_pw";
        assert_eq!(
            parse_csqtt_link(glued),
            Some(("example.com:46000".into(), "secret_pw".into()))
        );
        let prof = parse_csqtt_profile(glued).expect("glued profile");
        assert_eq!(prof.peer, "example.com:46000");
        assert_eq!(prof.password, "secret_pw");
        assert!(parse_csqtt_profile("csqtt://connect?v=2host=example.com&peer=46000&password=pwtoken=abc").is_none());
    }

    #[test]
    fn test_validate_and_normalize_peer() {
        assert!(validate_peer("1.2.3.4:46000").is_ok());
        assert!(validate_peer("vps.example.com:46000").is_ok());
        assert!(validate_peer(":46000").is_err());
        assert!(validate_peer("1.2.3.4:0").is_err());
        assert!(validate_peer("1.2.3.4:70000").is_err());
        assert!(validate_peer("a b:1").is_err());
        assert!(validate_peer("1.2.3.4").is_err());

        assert_eq!(normalize_peer("203.0.113.7").unwrap(), "203.0.113.7:46000", "PEER-порт csqtt по умолчанию — 46000");
        assert_eq!(normalize_peer(" vps.example.com:443 ").unwrap(), "vps.example.com:443");
        assert_eq!(normalize_peer("").unwrap(), "");
        assert!(normalize_peer("1.2.3.4;reboot").is_err());

        // Проверка парсинга ссылок csqtt://
        let (p1, pw1) = parse_csqtt_link("csqtt://connect?v=2&host=203.0.113.88&peer=46000&password=secret_pw").unwrap();
        assert_eq!(p1, "203.0.113.88:46000");
        assert_eq!(pw1, "secret_pw");

        let (p2, pw2) = parse_csqtt_link("csqtt://myPass@198.51.100.5:1234").unwrap();
        assert_eq!(p2, "198.51.100.5:1234");
        assert_eq!(pw2, "myPass");

        assert_eq!(normalize_peer("csqtt://connect?v=2&host=1.2.3.4&peer=46000&password=pw").unwrap(), "1.2.3.4:46000");
    }

    #[test]
    fn test_validate_password() {
        assert!(validate_password("abc").is_ok());
        assert!(validate_password("a|b").is_ok(), "пароль идёт в переменную окружения — | допустим");
        assert!(validate_password("").is_err());
        assert!(validate_password("a\nb").is_err());
        assert!(validate_password(&"x".repeat(129)).is_err());
    }

    #[test]
    fn test_effective_workers() {
        let mut cfg = cfg_with(&["https://vk.com/call/join/abc12345"]);
        assert_eq!(effective_workers(&cfg), 10, "10 workers per link (TRN-WRK-01)");
        cfg.links.push("https://vk.com/call/join/def67890".into());
        assert_eq!(effective_workers(&cfg), 20, "20 workers for 2 links");
        cfg.streams = 35;
        assert_eq!(effective_workers(&cfg), 35, "точное число воркеров");
        cfg.streams = 200;
        assert_eq!(effective_workers(&cfg), 100);
        cfg.streams = 1;
        assert_eq!(effective_workers(&cfg), 3);
    }

    /// Пароль не должен попадать в аргументы процесса (их видно в `ps` любому пользователю роутера).
    #[test]
    fn regression_tun_scripts_do_not_delete_foreign_policy_rules_or_flush_table() {
        let configure = tun_configure_script("10.66.67.3".parse().unwrap());
        let teardown = tun_teardown_script();
        for script in [&configure, &teardown] {
            assert!(!script.contains("ip rule del pref 40 2>"));
            assert!(!script.contains("ip rule del pref 45 2>"));
            assert!(!script.contains("ip rule del pref 222 2>"));
            assert!(!script.contains("ip route flush table 222"));
        }
        assert!(configure.contains("ip rule show"));
        assert!(configure.contains("ip route show table 222"));
    }

    #[tokio::test]
    async fn regression_failed_tun_setup_is_not_ready_and_same_config_retries() {
        let rt = RwLock::new(VkTurnRuntime::default());
        let ip = "10.66.67.3".parse().unwrap();
        let first = apply_tun_config(&rt, ip, None, |_| async { Err("route failed".into()) }).await;
        assert!(first.is_err());
        assert!(rt.read().await.tunnel_ip.is_none());
        let called = std::sync::atomic::AtomicBool::new(false);
        apply_tun_config(&rt, ip, None, |_| async { called.store(true, std::sync::atomic::Ordering::SeqCst); Ok(()) }).await.unwrap();
        assert!(called.load(std::sync::atomic::Ordering::SeqCst));
        assert_eq!(rt.read().await.tunnel_ip, Some(ip));
    }

    #[test]
    fn regression_handoff_retries_are_bounded_and_nonparallel() {
        assert_eq!(handoff_decision(true, false, true, 0, true), HandoffDecision::Wait);
        assert_eq!(handoff_decision(true, false, false, 1, false), HandoffDecision::Wait);
        assert_eq!(handoff_decision(true, false, false, 1, true), HandoffDecision::Start);
        assert_eq!(handoff_decision(true, false, false, 3, true), HandoffDecision::Restart);
        assert_eq!(handoff_decision(true, true, false, 0, true), HandoffDecision::Wait);
    }

    #[test]
    fn regression_handoff_retry_stops_after_cancellation() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let cancelled = AtomicBool::new(false);
        let mut calls = 0;
        let result = retry_fd_send(&cancelled, 3, Duration::ZERO, || {
            calls += 1;
            cancelled.store(true, Ordering::SeqCst);
            Err("unavailable".into())
        });
        assert!(result.is_err());
        assert_eq!(calls, 1);
    }

    #[test]
    fn regression_client_launches_use_distinct_tun_socket_names() {
        let cfg = cfg_with(&["abcdefghijklmnop"]);
        let first = build_client_args_with_hashes(&cfg, &call_hashes(&cfg));
        let second = build_client_args_with_hashes(&cfg, &call_hashes(&cfg));
        let socket = |args: &[String]| args[args.iter().position(|s| s == "-tun-uds").unwrap() + 1].clone();
        assert_ne!(socket(&first), socket(&second));
    }

    #[test]
    fn test_build_client_args_and_env() {
        let cfg = cfg_with(&["https://vk.com/call/join/abc12345", "https://vk.com/call/join/def67890"]);
        let args = build_client_args(&cfg);
        let uds_name = args[10].clone();
        assert!(uds_name.starts_with(&format!("{TUN_UDS_NAME}_")));
        assert_eq!(uds_name.len(), TUN_UDS_NAME.len() + 17);
        assert_eq!(
            args,
            vec![
                "-peer", "1.2.3.4:46000",
                "-vk", "abc12345,def67890",
                "-n", "20",
                "--exact-workers",
                "-listen", "127.0.0.1:0",
                "-tun-uds", uds_name.as_str(),
                "-captcha-mode", "auto",
                "-obfs", "audio",
                "-turn-transport", "udp",
            ]
        );
        assert!(!args.iter().any(|a| a.contains("s3cret-pass")), "пароль в аргументах процесса");
        let env = client_env(&cfg);
        assert!(env.contains(&("CSQTT_PASSWORD", "s3cret-pass".to_string())));
        assert!(env.contains(&("CSQTT_EVENTS", "1".to_string())));
        let dup = cfg_with(&["https://vk.com/call/join/abc12345", "abc12345"]);
        assert_eq!(call_hashes(&dup), vec!["abc12345"]);
    }

    #[test]
    fn test_vk_auth_parsing_and_bootstrap_formatting() {
        let url = "https://oauth.vk.ru/blank.html#access_token=vk1.a.TestTokenSecret123&user_id=12345678&expires_in=0";
        let (token, uid) = parse_vk_redirect_hash(url).expect("redirect url should be parsed");
        assert_eq!(token, "vk1.a.TestTokenSecret123");
        assert_eq!(uid, Some(12345678));

        let frag = "#access_token=vk1.a.AnotherToken&user_id=987654";
        let (token2, uid2) = parse_vk_redirect_hash(frag).expect("fragment should be parsed");
        assert_eq!(token2, "vk1.a.AnotherToken");
        assert_eq!(uid2, Some(987654));

        let raw = "vk1.a.RawDirectTokenOnly";
        let (token3, uid3) = parse_vk_redirect_hash(raw).expect("raw token should be accepted");
        assert_eq!(token3, "vk1.a.RawDirectTokenOnly");
        assert_eq!(uid3, None);

        assert!(parse_vk_redirect_hash("https://oauth.vk.ru/blank.html#error=access_denied").is_err());

        // Проверка формирования строки VK_JS_BOOTSTRAP
        let b_line = format_vk_bootstrap_line("vk1.a.SecretToken", 12345);
        assert!(b_line.starts_with("VK_JS_BOOTSTRAP:"));
        assert!(b_line.ends_with('\n'));
        let b64 = b_line.trim_start_matches("VK_JS_BOOTSTRAP:").trim();
        let decoded = crate::subscription_parser::base64_decode(b64).expect("valid base64");
        let json: serde_json::Value = serde_json::from_slice(&decoded).expect("valid json in bootstrap");
        assert_eq!(json["token"], "vk1.a.SecretToken");
        assert_eq!(json["access_token"], "vk1.a.SecretToken");
        assert_eq!(json["user_id"], 12345);

        // Проверка маскирования токена
        assert_eq!(mask_vk_token("vk1.a.SensitiveData"), "vk1.a.***");
        assert_eq!(mask_vk_token("other_token_secret"), "[REDACTED]");
        assert_eq!(mask_vk_token(""), "");

        // Проверка формирования аргументов csqtt с хешами auto_api (как в focsq)
        let oauth_cfg = cfg_with(&[]);
        let oauth_args = build_client_args_with_hashes(&oauth_cfg, &["autoGeneratedHash123".into()]);
        assert!(oauth_args.contains(&"-vk".to_string()));
        assert!(oauth_args.contains(&"autoGeneratedHash123".to_string()));
        assert!(oauth_args.contains(&"--exact-workers".to_string()));
    }

    #[test]
    fn test_turn_transport_fallback_and_tcp_mtu() {
        assert!(!should_fallback_to_tcp("auto", "udp", Duration::from_secs(10), 0, 5, false));
        assert!(should_fallback_to_tcp("auto", "udp", Duration::from_secs(25), 0, 2, false));
        assert!(should_fallback_to_tcp("auto", "udp", Duration::from_secs(30), 1, 9, false));
        assert!(!should_fallback_to_tcp("auto", "udp", Duration::from_secs(30), 5, 5, false));
        assert!(should_fallback_to_tcp("auto", "udp", Duration::from_secs(5), 5, 0, true));
        assert!(!should_fallback_to_tcp("udp", "udp", Duration::from_secs(30), 0, 5, true));
        assert!(!should_fallback_to_tcp("auto", "tcp", Duration::from_secs(30), 0, 5, true));

        let mut cfg = cfg_with(&["https://vk.com/call/join/abc12345", "https://vk.com/call/join/def67890"]);
        let tcp_args = build_client_args_for_uds_with_transport(&cfg, &call_hashes(&cfg), "sock1", "tcp");
        assert!(tcp_args.windows(2).any(|w| w == ["-turn-transport", "tcp"]));
        assert!(tcp_args.windows(2).any(|w| w == ["-n", "10"]), "TCP авто-кап должен ограничивать 20->10 потоков");
        cfg.streams = 16;
        assert_eq!(effective_workers_for_transport(&cfg, "tcp"), 16, "ручной выбор потоков сохраняется");

        assert_eq!(TUN_MTU, 900);
        assert_eq!(TUN_TCP_MSS, 840);
        assert_eq!(TUN_MTU_TCP, 1280);
        assert_eq!(TUN_TCP_MSS_TCP, 1220);
        assert!(TUN_MTU_TCP <= 1280, "MTU для TCP не должен превышать порог фрагментации csqttr (1280)");
        assert!(TUN_TCP_MSS_TCP + 52 <= 1280, "MSS + 52 (IPv4 + TCP с timestamps) должен укладываться в 1280");
        assert!(TUN_TCP_MSS_TCP + 60 <= TUN_MTU_TCP, "MSS + 60 должен укладываться в MTU TUN");
        assert!(tcp_args.windows(2).any(|w| w == ["-obfs", "audio"]));

        let ip: std::net::Ipv4Addr = "10.66.67.2".parse().unwrap();
        let tcp_script = tun_configure_script_for_transport(ip, "tcp");
        assert!(tcp_script.contains("mtu 1280 up"));
        assert!(!tcp_script.contains("mtu 1360 up"));
        assert!(tcp_script.contains("-I \"$ch\" 1 -o csq0 -p tcp --tcp-flags SYN,RST SYN -m comment --comment xkeen-route-vkturn -j TCPMSS --set-mss 1220"));
        assert!(tcp_script.contains("-I \"$ch\" 1 -i csq0 -p tcp --tcp-flags SYN,RST SYN -m comment --comment xkeen-route-vkturn -j TCPMSS --set-mss 1220"));
        assert!(!tcp_script.contains("-I \"$ch\" 1 -o csq0 -p tcp --tcp-flags SYN,RST SYN -m comment --comment xkeen-route-vkturn -j TCPMSS --set-mss 1300"));
        assert!(!tcp_script.contains("-I \"$ch\" 1 -i csq0 -p tcp --tcp-flags SYN,RST SYN -m comment --comment xkeen-route-vkturn -j TCPMSS --set-mss 1300"));
        assert!(tcp_script.contains("-D \"$ch\" -o csq0 -p tcp --tcp-flags SYN,RST SYN -m comment --comment xkeen-route-vkturn -j TCPMSS --set-mss 840"));
        assert!(tcp_script.contains("-D \"$ch\" -i csq0 -p tcp --tcp-flags SYN,RST SYN -m comment --comment xkeen-route-vkturn -j TCPMSS --set-mss 840"));
        assert!(tcp_script.contains("-D \"$ch\" -o csq0 -p tcp --tcp-flags SYN,RST SYN -m comment --comment xkeen-route-vkturn -j TCPMSS --set-mss 1300"));
        assert!(tcp_script.contains("-D \"$ch\" -i csq0 -p tcp --tcp-flags SYN,RST SYN -m comment --comment xkeen-route-vkturn -j TCPMSS --set-mss 1300"));

        let udp_script = tun_configure_script_for_transport(ip, "udp");
        assert!(udp_script.contains("mtu 900 up"));
        assert!(udp_script.contains("-I \"$ch\" 1 -o csq0 -p tcp --tcp-flags SYN,RST SYN -m comment --comment xkeen-route-vkturn -j TCPMSS --set-mss 840"));
        assert!(udp_script.contains("-I \"$ch\" 1 -i csq0 -p tcp --tcp-flags SYN,RST SYN -m comment --comment xkeen-route-vkturn -j TCPMSS --set-mss 840"));
        assert!(udp_script.contains("-D \"$ch\" -o csq0 -p tcp --tcp-flags SYN,RST SYN -m comment --comment xkeen-route-vkturn -j TCPMSS --set-mss 1220"));
        assert!(udp_script.contains("-D \"$ch\" -i csq0 -p tcp --tcp-flags SYN,RST SYN -m comment --comment xkeen-route-vkturn -j TCPMSS --set-mss 1220"));
        assert!(udp_script.contains("-D \"$ch\" -o csq0 -p tcp --tcp-flags SYN,RST SYN -m comment --comment xkeen-route-vkturn -j TCPMSS --set-mss 1300"));
        assert!(udp_script.contains("-D \"$ch\" -i csq0 -p tcp --tcp-flags SYN,RST SYN -m comment --comment xkeen-route-vkturn -j TCPMSS --set-mss 1300"));

        let teardown = tun_teardown_script();
        for mss in [840, 1220, 1300] {
            assert!(teardown.contains(&format!("-D \"$ch\" -o csq0 -p tcp --tcp-flags SYN,RST SYN -m comment --comment xkeen-route-vkturn -j TCPMSS --set-mss {mss}")));
            assert!(teardown.contains(&format!("-D \"$ch\" -i csq0 -p tcp --tcp-flags SYN,RST SYN -m comment --comment xkeen-route-vkturn -j TCPMSS --set-mss {mss}")));
        }

        let mut rt = VkTurnRuntime::default();
        let now = Instant::now();
        apply_log_line(&mut rt, r#"__CSQTT_STREAM__|{"stage":"dtls","status":"ok","worker":1}"#, now);
        apply_log_line(&mut rt, r#"__CSQTT_STREAM__|{"stage":"dtls","status":"error","worker":2,"reason":"timeout"}"#, now);
        apply_log_line(&mut rt, r#"__CSQTT_STREAM__|{"stage":"dtls","status":"error","worker":3,"reason":"timeout"}"#, now);
        assert_eq!(rt.dtls_ok_streams, 1);
        assert_eq!(rt.dtls_fallback_streams, 2);
    }

    #[test]
    fn test_with_primary_link() {
        let links = vec![
            "https://vk.com/call/join/oldmain1234".to_string(),
            "https://vk.com/call/join/backup5678".to_string(),
        ];
        let out = with_primary_link(&links, "  https://vk.com/call/join/fresh12345/ ").unwrap();
        assert_eq!(out, vec![
            "https://vk.com/call/join/fresh12345".to_string(),
            "https://vk.com/call/join/oldmain1234".to_string(),
            "https://vk.com/call/join/backup5678".to_string(),
        ]);
        let out2 = with_primary_link(&out, "backup5678").unwrap();
        assert_eq!(out2.len(), 3);
        assert_eq!(out2[0], "https://vk.com/call/join/backup5678");
        let many: Vec<String> = (0..10).map(|i| format!("https://vk.com/call/join/link{i:06}")).collect();
        assert_eq!(with_primary_link(&many, "https://vk.com/call/join/newlink000").unwrap().len(), MAX_LINKS);
        assert!(with_primary_link(&links, "https://telemost.yandex.ru/j/12345678").is_err());
    }

    // Строки ниже — формат событий и сообщений из исходников клиента csqtt (events.rs, worker.rs, protocol.rs).
    const EV_READY: &str = r#"__CSQTT_EVENT__|READY|{"worker":1}"#;
    const EV_STATS_3: &str = r#"__CSQTT_EVENT__|STATS|{"active":3,"bytes_down":2048,"bytes_up":1024}"#;
    const EV_STATS_0: &str = r#"__CSQTT_EVENT__|STATS|{"active":0,"bytes_down":2048,"bytes_up":1024}"#;
    const EV_AUTH: &str = r#"__CSQTT_EVENT__|ERROR|{"code":"authentication_rejected","fatal":true,"message":"FATAL_AUTH: invalid password"}"#;

    #[test]
    fn regression_tun_lost_requests_replacement_while_client_runs() {
        let mut rt = VkTurnRuntime::default();
        let ip = "10.66.67.3".parse().unwrap();
        rt.tunnel_ip = Some(ip);
        rt.tunnel_dns = Some("1.1.1.1".into());
        rt.tun_attached = true;
        let result = apply_log_line(&mut rt, r#"__CSQTT_EVENT__|TUN_LOST|{}"#, Instant::now());
        assert_eq!(result, LineEffect::TunConfig { ip, dns: Some("1.1.1.1".into()) });
        assert!(!rt.tun_attached);
    }

    #[test]
    fn distinguishes_csqtt_vk_and_timeout_error_sources() {
        let mut rt = VkTurnRuntime::default();
        apply_log_line(&mut rt, EV_AUTH, Instant::now());
        assert_eq!((rt.error_source, rt.error_reason), (Some("csqtt"), Some("password_invalid")));
        apply_log_line(&mut rt, "User authorization failed", Instant::now());
        assert_eq!((rt.error_source, rt.error_reason), (Some("vk"), Some("authorization_rejected")));
        rt.phase = VkTurnPhase::Starting;
        rt.connected_streams = 0;
        for _ in 0..GETCONF_SILENT_THRESHOLD { note_getconf_silent(&mut rt); }
        assert_eq!((rt.error_source, rt.error_reason), (Some("network"), Some("server_timeout")));
    }

    #[tokio::test]
    async fn regression_stale_log_reader_cannot_change_new_status() {
        let manager = VkTurnManager::with_core_path("unused");
        let (reader, mut writer) = tokio::io::duplex(1024);
        let (sender, receiver) = mpsc::unbounded_channel();
        let task = manager.spawn_log_reader(Box::new(reader), sender);
        let mut rt = manager.rt.write().await;
        rt.phase = VkTurnPhase::Starting;
        writer.write_all(format!("{EV_AUTH}\n").as_bytes()).await.unwrap();
        tokio::task::yield_now().await; // читатель может ждать блокировку состояния
        drop(receiver); // завершённый запуск больше не имеет получателя событий
        drop(rt);
        drop(writer);
        tokio::time::timeout(Duration::from_secs(2), task).await.unwrap().unwrap();
        assert_eq!(manager.rt.read().await.phase, VkTurnPhase::Starting);
    }

    #[tokio::test]
    async fn regression_stale_handoff_completion_cannot_mark_new_run_ready() {
        let manager = VkTurnManager::with_core_path("unused");
        let old_generation = manager.begin_run().await;
        let (release, delayed) = tokio::sync::oneshot::channel();
        let old_manager = manager.clone();
        let old_task = tokio::spawn(async move {
            let _ = delayed.await;
            old_manager.publish_handoff_result(old_generation, Ok(())).await;
        });
        manager.stop().await;
        let new_generation = manager.begin_run().await;
        assert_ne!(old_generation, new_generation);
        release.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(2), old_task).await.unwrap().unwrap();
        assert!(!manager.rt.read().await.tun_attached);
        manager.publish_handoff_result(new_generation, Ok(())).await;
        manager.publish_handoff_result(old_generation, Err("old failure".into())).await;
        let rt = manager.rt.read().await;
        assert!(rt.tun_attached);
        assert!(rt.last_error.is_none());
    }
    const L_CREDS: &str = "[ГРУППА #1] Запрос кредов (хеш: dVi6izW6...)";
    const L_VKCALLS: &str = "[STREAM 100] [VKCalls] step1 OK, anonymous_token (290 chars)";
    const L_CAPTCHA_START: &str = "[STREAM 100] [КАПЧА] AUTO: старт цепочки (captcha attempt 1)";
    const L_CAPTCHA_FAIL: &str = "[ГРУППА #1] Ошибка кредов: all VK credentials failed: CAPTCHA_WAIT_REQUIRED";
    const L_AUTH_WRONG: &str = "[ВОРКЕР #1] Фатальная ошибка: FATAL_AUTH: неверный пароль подключения";
    const L_AUTH_EXPIRED: &str = "[ВОРКЕР #1] Фатальная ошибка: FATAL_AUTH: срок действия пароля истёк";
    const L_WRAP_TIMEOUT: &str = "[ВОРКЕР #2] [HANDSHAKE][RETRY] WRAP_AUTH_TIMEOUT, 3 попыток восстановления";
    const L_CALL_DEAD: &str = "[ГРУППА #1] Ошибка кредов: VK returns error: Call not found (error_code=9000)";
    const L_TURN_URL: &str = "[VKCalls] turn_server.urls[0]=95.163.34.143:19302";

    #[test]
    fn events_drive_streams_and_traffic() {
        let mut rt = VkTurnRuntime::default();
        let now = Instant::now();
        assert_eq!(apply_log_line(&mut rt, L_CREDS, now), LineEffect::None);
        assert_eq!(rt.phase, VkTurnPhase::Authorizing);
        apply_log_line(&mut rt, EV_READY, now);
        assert_eq!(rt.phase, VkTurnPhase::Connected);
        assert_eq!(rt.connected_streams, 1);
        apply_log_line(&mut rt, EV_STATS_3, now);
        assert_eq!(rt.connected_streams, 3);
        assert_eq!((rt.bytes_up, rt.bytes_down), (1024, 2048));
        apply_log_line(&mut rt, EV_STATS_0, now);
        assert_eq!(rt.connected_streams, 0);
        assert_eq!(rt.phase, VkTurnPhase::Degraded);
        assert!(rt.zero_streams_since.is_some());
        assert!(rt.log_tail.iter().all(|l| !l.contains("__CSQTT_EVENT__")), "машинные события не засоряют лог");
    }

    /// Адрес туннеля от сервера (protocol.rs: `TUNCONF:{ip}:{dns}:0:stream-v1`, бывает с хвостом pad=…).
    #[test]
    fn tunconf_event_returns_tunnel_address() {
        let mut rt = VkTurnRuntime::default();
        let ev = r#"__CSQTT_EVENT__|CONFIG|{"config":"TUNCONF:10.66.67.3:1.1.1.1:0:stream-v1"}"#;
        assert_eq!(
            apply_log_line(&mut rt, ev, Instant::now()),
            LineEffect::TunConfig { ip: "10.66.67.3".parse().unwrap(), dns: Some("1.1.1.1".into()) }
        );
        assert_eq!(
            parse_tunconf("TUNCONF:10.66.67.9:8.8.8.8,1.1.1.1:0:stream-v1:pad=xxxx"),
            Some(("10.66.67.9".parse().unwrap(), Some("8.8.8.8,1.1.1.1".into())))
        );
        assert_eq!(parse_tunconf("TUNCONF:10.66.67.9:;reboot:0"), Some(("10.66.67.9".parse().unwrap(), None)), "мусор в DNS отбрасывается");
        assert_eq!(parse_tunconf("TUNCONF:0.0.0.0:1.1.1.1"), None);
        assert_eq!(parse_tunconf("TUNCONF:10.66.67.3;id:1.1.1.1"), None, "инъекция в адрес невозможна");
        assert_eq!(parse_tunconf("GARBAGE"), None);
        // Туннель «готов» только когда и адрес получен, и TUN передан клиенту
        rt.tunnel_ip = Some("10.66.67.3".parse().unwrap());
        assert!(!rt.tunnel_ready());
        rt.tun_attached = true;
        assert!(!rt.tunnel_ready());
        rt.tun_configured = true;
        assert!(rt.tunnel_ready());
    }

    #[test]
    fn tun_scripts_only_use_constants_and_parsed_ip() {
        let s = tun_configure_script("10.66.67.3".parse().unwrap());
        assert!(s.contains("ip addr add 10.66.67.3/32 dev csq0"));
        assert!(s.contains("ip link set dev csq0 mtu 900 up"));
        assert!(s.contains("TCPMSS --set-mss 840"));
        assert!(s.contains("-o csq0 -p udp --dport 443 -m comment --comment xkeen-route-vkturn -j REJECT"));
        assert!(s.contains("ip route add default dev csq0 table 222"));
        assert!(s.contains("ip rule add dev csq0 table 222 pref 222"));
        assert!(s.contains("ip rule del pref 222"), "правило не дублируется при повторной настройке");
        assert!(s.contains("ip rule add from 10.66.67.3 table 222 pref 40"));
        assert!(s.contains("ip rule add fwmark 222 table 222 pref 45"));
        let t = tun_teardown_script_for_ip(Some("10.66.67.3".parse().unwrap()));
        assert!(t.contains("TCPMSS --set-mss 840"));
        assert!(t.contains("-o csq0 -p udp --dport 443 -m comment --comment xkeen-route-vkturn -j REJECT"));
        assert!(t.contains("ip rule del pref 222 dev csq0 table 222"));
        assert!(t.contains("ip route del default dev csq0 table 222"));
        assert!(t.contains("ip rule del pref 40 from 10.66.67.3 table 222"));
        assert!(t.contains("ip rule del pref 45 fwmark 222 table 222"));
    }

    #[test]
    fn captcha_solve_request_is_answered_and_token_hidden() {
        let mut rt = VkTurnRuntime::default();
        let now = Instant::now();
        let req = "CAPTCHA_SOLVE|auto|https://id.vk.ru/not_robot_captcha?session_token=FAKE_SESSION_TOKEN_1|FAKE_SESSION_TOKEN_1";
        assert_eq!(apply_log_line(&mut rt, req, now), LineEffect::CaptchaSolve);
        assert_eq!(rt.phase, VkTurnPhase::SolvingCaptcha);
        assert!(rt.log_tail.iter().all(|l| !l.contains("FAKE_SESSION_TOKEN_1")));
        assert!(CAPTCHA_NO_WEBVIEW_REPLY.starts_with("CAPTCHA_RESULT|error:"), "клиент ждёт error:… чтобы сразу идти дальше");
        assert!(CAPTCHA_NO_WEBVIEW_REPLY.ends_with('\n'));
    }

    #[test]
    fn captcha_failure_is_sticky_until_connected() {
        let mut rt = VkTurnRuntime::default();
        let now = Instant::now();
        apply_log_line(&mut rt, L_VKCALLS, now);
        apply_log_line(&mut rt, L_CAPTCHA_START, now);
        assert_eq!(rt.phase, VkTurnPhase::SolvingCaptcha);
        apply_log_line(&mut rt, L_CAPTCHA_FAIL, now);
        assert_eq!(rt.phase, VkTurnPhase::CaptchaRequired);
        assert!(rt.last_error.as_deref().unwrap().contains("не робот"));
        apply_log_line(&mut rt, L_CREDS, now);
        apply_log_line(&mut rt, L_CAPTCHA_START, now);
        assert_eq!(rt.phase, VkTurnPhase::CaptchaRequired);
        apply_log_line(&mut rt, EV_READY, now);
        assert_eq!(rt.phase, VkTurnPhase::Connected);
        assert!(rt.last_error.is_none());
    }

    // Строки клиента csqtt с роутера: DTLS с сервером проходит, а GETCONF с неверным паролем остаётся без ответа.
    const S_DTLS_OK: &str = r#"__CSQTT_STREAM__|{"attempt":177,"elapsed_ms":164,"reason":"DTLS-SRTP established","sequence":1939,"stage":"dtls","status":"ok","time_ms":1793949028092,"transport":"UDP","turn":"95.163.34.139:19302","worker":16}"#;
    const S_GETCONF_START: &str = r#"__CSQTT_STREAM__|{"attempt":177,"elapsed_ms":0,"reason":"","sequence":1940,"stage":"getconf","status":"start","time_ms":1793949028092,"transport":"UDP","turn":"95.163.34.139:19302","worker":16}"#;
    const S_GETCONF_TIMEOUT: &str = r#"__CSQTT_STREAM__|{"attempt":160,"elapsed_ms":5804,"reason":"GETCONF чтение ответа конфига: timeout после 4 попыток","sequence":1950,"stage":"getconf","status":"error","time_ms":1793949030728,"transport":"UDP","turn":"95.163.34.176:19302","worker":5}"#;
    const S_GETCONF_OK: &str = r#"__CSQTT_STREAM__|{"attempt":1,"elapsed_ms":90,"reason":"","sequence":20,"stage":"getconf","status":"ok","time_ms":1793949030728,"transport":"UDP","turn":"95.163.34.176:19302","worker":2}"#;

    #[test]
    fn regression_vk_getconf_timeouts_do_not_prove_wrong_password() {
        let now = Instant::now();
        let mut rt = VkTurnRuntime::default();
        rt.phase = VkTurnPhase::Authorizing;
        for _ in 0..3 {
            apply_log_line(&mut rt, S_GETCONF_TIMEOUT, now);
        }
        assert_eq!(rt.phase, VkTurnPhase::ServerUnreachable);
        assert!(rt.last_error.as_deref().unwrap().contains("пароль"));
        apply_log_line(&mut rt, "[DTLS] retry", now);
        assert_eq!(rt.phase, VkTurnPhase::ServerUnreachable);
        apply_log_line(&mut rt, EV_READY, now);
        assert_eq!(rt.phase, VkTurnPhase::Connected);
    }

    // Регрессия: при неверном пароле сервер csqtt молчит на GETCONF, а панель ждала строку
    // WRAP_AUTH_TIMEOUT прежнего ядра — статус навсегда оставался «Подключаюсь…» без поля для пароля.
    #[test]
    fn silent_getconf_after_dtls_reports_unreachable_server() {
        let now = Instant::now();
        let mut rt = VkTurnRuntime::default();
        rt.phase = VkTurnPhase::Authorizing;
        apply_log_line(&mut rt, S_DTLS_OK, now);
        apply_log_line(&mut rt, S_GETCONF_START, now);
        apply_log_line(&mut rt, S_GETCONF_TIMEOUT, now);
        apply_log_line(&mut rt, S_GETCONF_TIMEOUT, now);
        assert_eq!(rt.phase, VkTurnPhase::Authorizing, "одна-две потери пакетов — ещё не повод");
        apply_log_line(&mut rt, S_GETCONF_TIMEOUT, now);
        assert_eq!(rt.phase, VkTurnPhase::ServerUnreachable);
        assert!(rt.last_error.as_deref().unwrap().contains("пароль"));
        assert!(is_sticky(rt.phase), "фаза держится и показывает поле для нового пароля");

        // Журнал: успешные этапы не засоряют его JSON-ом, ошибки — коротко
        assert!(rt.log_tail.iter().all(|l| !l.contains("__CSQTT_STREAM__")));
        assert!(rt.log_tail.iter().any(|l| l == "[ПОТОК #5] getconf: GETCONF чтение ответа конфига: timeout после 4 попыток"));
    }

    #[test]
    fn getconf_answer_resets_silence_counter() {
        let now = Instant::now();
        let mut rt = VkTurnRuntime::default();
        rt.phase = VkTurnPhase::Authorizing;
        apply_log_line(&mut rt, S_GETCONF_TIMEOUT, now);
        apply_log_line(&mut rt, S_GETCONF_TIMEOUT, now);
        apply_log_line(&mut rt, S_GETCONF_OK, now);
        apply_log_line(&mut rt, S_GETCONF_TIMEOUT, now);
        apply_log_line(&mut rt, S_GETCONF_TIMEOUT, now);
        assert_eq!(rt.phase, VkTurnPhase::Authorizing, "сервер ответил — пароль верный, редкие потери не считаются");

        // При живых потоках потерянный GETCONF не объявляет пароль неверным
        let mut rt2 = VkTurnRuntime::default();
        rt2.connected_streams = 3;
        rt2.phase = VkTurnPhase::Connected;
        for _ in 0..5 {
            apply_log_line(&mut rt2, S_GETCONF_TIMEOUT, now);
        }
        assert_eq!(rt2.phase, VkTurnPhase::Connected);
    }

    #[test]
    fn server_auth_errors_are_recognized() {
        let now = Instant::now();
        let mut rt = VkTurnRuntime::default();
        apply_log_line(&mut rt, L_AUTH_WRONG, now);
        assert_eq!(rt.phase, VkTurnPhase::AuthFailed);
        assert!(rt.last_error.as_deref().unwrap().contains("неверный"));
        apply_log_line(&mut rt, L_WRAP_TIMEOUT, now);
        assert_eq!(rt.phase, VkTurnPhase::AuthFailed, "таймаут после отказа не меняет понятную причину");

        let mut rt2 = VkTurnRuntime::default();
        apply_log_line(&mut rt2, L_AUTH_EXPIRED, now);
        assert!(rt2.last_error.as_deref().unwrap().contains("истёк"));

        let mut rt3 = VkTurnRuntime::default();
        apply_log_line(&mut rt3, L_WRAP_TIMEOUT, now);
        assert_eq!(rt3.phase, VkTurnPhase::ServerUnreachable);
        assert!(rt3.last_error.as_deref().unwrap().contains("пароль"), "при неверном пароле сервер молчит — подсказываем");

        let mut rt4 = VkTurnRuntime::default();
        apply_log_line(&mut rt4, EV_AUTH, now);
        assert_eq!(rt4.phase, VkTurnPhase::AuthFailed, "машинное событие ERROR authentication_rejected");
        assert!(rt4.last_error.as_deref().unwrap().contains("неверный"));
    }

    #[test]
    fn dead_call_link_is_recognized_but_does_not_hide_working_streams() {
        let now = Instant::now();
        let mut rt = VkTurnRuntime::default();
        apply_log_line(&mut rt, L_CALL_DEAD, now);
        assert_eq!(rt.phase, VkTurnPhase::LinkInvalid);

        let mut rt2 = VkTurnRuntime::default();
        apply_log_line(&mut rt2, EV_STATS_3, now);
        apply_log_line(&mut rt2, L_CALL_DEAD, now);
        assert_eq!(rt2.phase, VkTurnPhase::Connected);
        assert!(rt2.last_error.is_some());
    }

    /// Регрессия WDTT: адреса TURN ВК в реальном логе идут без префикса `turn:` (95.163.34.143:19302) —
    /// старый разбор их не видел, и исключения в iptables ставились только на подсеть 155.212.192.0/20.
    #[test]
    fn turn_addresses_are_learned_for_exemptions() {
        let mut rt = VkTurnRuntime::default();
        let now = Instant::now();
        apply_log_line(&mut rt, L_TURN_URL, now);
        apply_log_line(&mut rt, "[ГРУППА #1] Креды OK, TURN: [95.163.34.143:19302 95.163.34.131:19302], 10 воркеров", now);
        apply_log_line(&mut rt, "[СЕССИЯ #1] Relay: 95.163.34.131:62836", now);
        apply_log_line(&mut rt, "[TURN] Подключение к 127.0.0.1:3478", now);
        apply_log_line(&mut rt, "[TURN] Подключение к 10.0.0.1:3478", now);
        assert_eq!(
            rt.turn_ips.iter().map(|i| i.to_string()).collect::<Vec<_>>(),
            vec!["95.163.34.131", "95.163.34.143"]
        );
        for i in 0..100u8 {
            apply_log_line(&mut rt, &format!("[TURN] Подключение к 198.51.{i}.1:3478"), now);
        }
        assert!(rt.turn_ips.len() <= MAX_TURN_IPS, "список адресов TURN ограничен");
    }

    #[test]
    fn log_tail_is_bounded_and_redacted() {
        let mut rt = VkTurnRuntime::default();
        for i in 0..400 {
            apply_log_line(&mut rt, &format!("log line {i}"), Instant::now());
        }
        assert_eq!(rt.log_tail.len(), LOG_TAIL);

        let r = redact_line("join https://vk.com/call/join/SecretCallToken123456789 ok");
        assert!(!r.contains("SecretCallToken123456789"));
        assert!(r.contains("call/join/Secr…"));
        let r = redact_line("x?session_token=abc123def456&variant=popup token=zzz999yyy888");
        assert!(!r.contains("abc123def456") && !r.contains("zzz999yyy888"));
        assert!(r.contains("&variant=popup"));
        let r = redact_line("[КЛИЕНТ] Запрос кредов (хеш: dVi6izW61Ty9UuE1hEkU)");
        assert!(!r.contains("dVi6izW61Ty9UuE1hEkU"), "хеш звонка скрыт: {r}");
    }

    #[test]
    fn stall_limits() {
        assert_eq!(stall_limit(VkTurnPhase::AuthFailed), Duration::from_secs(15));
        assert_eq!(stall_limit(VkTurnPhase::LinkInvalid), Duration::from_secs(20));
        assert_eq!(stall_limit(VkTurnPhase::CaptchaRequired), Duration::from_secs(45));
        assert_eq!(stall_limit(VkTurnPhase::SolvingCaptcha), Duration::from_secs(300));
        assert_eq!(stall_limit(VkTurnPhase::Authorizing), Duration::from_secs(120));
    }

    #[test]
    fn test_render_provider_yaml() {
        let yaml = render_provider_yaml();
        assert!(yaml.contains(PROXY_NAME));
        assert!(yaml.contains("[БС]"));
        assert!(yaml.contains("type: direct"));
        assert!(yaml.contains("interface-name: csq0"), "трафик узла уходит в TUN клиента csqtt");
        assert!(yaml.contains("routing-mark: 222"));
        assert!(yaml.contains("ip-version: ipv4"));
        assert!(yaml.contains("udp: false"));
        assert!(yaml.contains("tfo: false"));
    }

    #[test]
    fn test_apply_and_remove_rules() {
        let yaml = "port: 7890\nrules:\n  - MATCH,DIRECT\n";
        let mut cfg = cfg_with(&["https://vk.com/call/join/abc12345"]);

        let applied = apply_rules(yaml, &cfg).expect("apply rules");
        assert!(applied.contains(RULES_BEGIN));
        assert!(applied.contains("IP-CIDR,155.212.192.0/20,DIRECT,no-resolve"));
        assert!(applied.contains("IP-CIDR,1.2.3.4/32,DIRECT,no-resolve"));
        assert_eq!(applied, apply_rules(&applied, &cfg).unwrap(), "идемпотентно");

        cfg.enabled = false;
        let disabled = apply_rules(&applied, &cfg).expect("disable rules");
        assert!(!disabled.contains(RULES_BEGIN));
        assert_eq!(disabled.trim(), yaml.trim());

        cfg.enabled = true;
        assert!(apply_rules("port: 7890\r\nrules:\r\n  - MATCH,DIRECT\r\n", &cfg).unwrap().contains("\r\n"));
        assert!(apply_rules("port: 7890\nproxies:\n", &cfg).is_err());
    }

    #[test]
    fn test_core_asset_for_arch() {
        assert_eq!(core_asset_for_arch("arm64"), Some("csqtt-client-linux-arm64"));
        assert_eq!(core_asset_for_arch("aarch64"), Some("csqtt-client-linux-arm64"));
        assert_eq!(core_asset_for_arch("mipsle-softfloat"), None, "для MIPS сборки csqtt нет — понятная ошибка");
        assert_eq!(core_asset_for_arch("unknown_arch"), None);
        assert_eq!(elf_target_for_arch("mipsle-softfloat"), "mipsel");
        assert_eq!(elf_target_for_arch("arm64"), "aarch64");
        assert_eq!(elf_target_for_arch("armv7"), "arm");
    }

    /// Каждой архитектуре из сборки соответствует проверенный SHA-256 (64 hex), без заглушек.
    #[test]
    fn every_core_asset_has_a_real_hash() {
        for arch in ["arm64"] {
            let asset = core_asset_for_arch(arch).unwrap();
            let hash = CORE_SHA256.iter().find(|(a, _)| *a == asset).map(|(_, h)| *h);
            let hash = hash.unwrap_or_else(|| panic!("нет хеша для {asset}"));
            assert_eq!(hash.len(), 64);
            assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
        }
        assert!(CORE_TAG.ends_with(&CORE_UPSTREAM_COMMIT[..7]), "тег релиза ядра соответствует коммиту сборки");
    }

    /// Пароль сервера csqtt и токен VK — секреты: в API настроек они скрыты, а «******» при сохранении не затирает настоящий.
    #[test]
    fn settings_api_masks_and_preserves_tunnel_password() {
        let mut cfg = crate::config::AppConfig::default();
        cfg.vkturn.password = "top-secret-pw".into();
        cfg.vkturn.vk_token = Some("vk1.a.VerySecretTokenLongString".into());
        let masked = crate::api::mask_settings_for_api(&cfg);
        assert_eq!(masked["vkturn"]["password"], "******");
        assert_eq!(masked["vkturn"]["vk_token"], "vk1.a.***");
        assert!(!masked.to_string().contains("top-secret-pw"));
        assert!(!masked.to_string().contains("VerySecretTokenLongString"));
        let merged = crate::api::merge_settings_update(&cfg, &masked).expect("merge masked settings");
        assert_eq!(merged.vkturn.password, "top-secret-pw");
    }

    #[tokio::test]
    async fn regression_vk_unavailable_does_not_replace_existing_account() {
        let (root, mut state) = setup_saved_state("auth-offline").await;
        state.http = reqwest::Client::builder()
            .proxy(reqwest::Proxy::all("http://127.0.0.1:1").unwrap())
            .timeout(Duration::from_secs(2)).build().unwrap();
        commit_vkturn(&state, |v| { v.vk_token = Some("vk1.a.PreviousSyntheticToken".into()); Ok(()) }).await.unwrap();
        let req = crate::api::VkTurnAuthSaveReq { token: Some("vk1.a.NewSyntheticToken".into()), ..Default::default() };
        let result = response_json(crate::api::save_vkturn_auth(axum::extract::State(state.clone()), axum::Json(req)).await).await;
        assert_eq!(result["success"], false);
        assert_eq!(state.config.read().await.vkturn.vk_token.as_deref(), Some("vk1.a.PreviousSyntheticToken"));
        commit_vkturn(&state, |v| { v.vk_token = None; Ok(()) }).await.unwrap();
        let req = crate::api::VkTurnAuthSaveReq { token: Some("vk1.a.NewSyntheticToken".into()), ..Default::default() };
        let result = response_json(crate::api::save_vkturn_auth(axum::extract::State(state.clone()), axum::Json(req)).await).await;
        assert_eq!(result["success"], true);
        assert_eq!(result["data"]["verified"], false);
        assert!(!state.config.read().await.vkturn.vk_token_verified);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn regression_vk_rejected_permission_code_is_never_saved() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            loop {
                let Ok((mut stream, _)) = listener.accept().await else { break; };
                tokio::spawn(async move {
                    let mut buf = [0u8; 2048];
                    let n = stream.read(&mut buf).await.unwrap_or(0);
                    let req_str = String::from_utf8_lossy(&buf[..n]);
                    if req_str.contains("account.getAppPermissions") {
                        let body = r#"{"error":{"error_code":27,"error_msg":"Group authorization failed: method is unavailable with group auth."}}"#;
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        let _ = stream.write_all(resp.as_bytes()).await;
                    } else {
                        let resp = "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                        let _ = stream.write_all(resp.as_bytes()).await;
                    }
                });
            }
        });

        let (root, mut state) = setup_saved_state("auth-rejected-code27").await;
        state.http = reqwest::Client::builder()
            .resolve("api.vk.ru", addr)
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();

        let err = fetch_vk_user_profile_at(
            &state.http,
            &format!("http://{addr}"),
            "vk1.a.InvalidGroupTokenSynthetic",
            Some(12345),
        )
        .await;
        assert_eq!(err, Err(VkProfileError::Rejected));
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn test_vk_auth_endpoints_save_and_logout() {
        let (root, mut state) = setup_saved_state("auth").await;
        state.http = reqwest::Client::builder()
            .proxy(reqwest::Proxy::all("http://127.0.0.1:1").unwrap())
            .timeout(Duration::from_secs(2)).build().unwrap();

        // 1. Сохранение авторизации через фрагмент URL
        let req = crate::api::VkTurnAuthSaveReq {
            url: Some("https://oauth.vk.ru/blank.html#access_token=vk1.a.UnitTestSecretToken&user_id=88889999".into()),
            auth_mode: Some("oauth".into()),
            ..Default::default()
        };
        let resp = crate::api::save_vkturn_auth(axum::extract::State(state.clone()), axum::Json(req)).await;
        let v = response_json(resp).await;
        assert_eq!(v["success"], true);
        assert_eq!(v["data"]["user_id"], 88889999);
        assert_eq!(v["data"]["verified"], false);

        let cur_cfg = state.config.read().await.vkturn.clone();
        assert_eq!(cur_cfg.vk_token.as_deref(), Some("vk1.a.UnitTestSecretToken"));
        assert_eq!(cur_cfg.vk_user_id, Some(88889999));
        assert_eq!(cur_cfg.auth_mode, "oauth");

        // 2. Проверка статуса API на отдачу маскированного токена и профиля
        let st_resp = crate::api::get_vkturn_status(axum::extract::State(state.clone())).await;
        let st = response_json(st_resp).await;
        assert_eq!(st["data"]["has_token"], true);
        assert_eq!(st["data"]["vk_token"], "vk1.a.***");
        assert_eq!(st["data"]["vk_user_id"], 88889999);

        // 3. Выход из аккаунта
        let logout_resp = crate::api::logout_vkturn_auth(axum::extract::State(state.clone())).await;
        let lv = response_json(logout_resp).await;
        assert_eq!(lv["success"], true);

        let logged_out_cfg = state.config.read().await.vkturn.clone();
        assert!(logged_out_cfg.vk_token.is_none());
        assert!(logged_out_cfg.vk_user_id.is_none());
        assert!(logged_out_cfg.vk_user_name.is_none());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_is_whitelist_server_full_vkturn() {
        let mut cfg = crate::config::AppConfig::default();
        cfg.whitelist_providers.push(PROVIDER_ID.to_string());
        assert!(crate::failover::is_whitelist_server_full(PROXY_NAME, "any_id", Some(PROVIDER_ID), &cfg));
    }

    // ---------- Анти-регрессионные тесты ----------

    #[tokio::test]
    async fn regression_vk_switch_rejects_unready_tunnel_before_contacting_mihomo() {
        let root = std::env::temp_dir();
        let mgr = VkTurnManager::with_core_path(root.join("nonexistent-csqtt"));
        let state = test_state(&root, mgr, 1);
        let error = crate::api::execute_server_switch_workflow(&state, PROXY_NAME).await.unwrap_err();
        assert!(error.contains("Туннель ещё не подключился"), "unexpected error: {error}");
    }

    #[tokio::test]
    async fn regression_vk_switch_rejects_tun_without_connected_streams() {
        let root = std::env::temp_dir();
        let mgr = VkTurnManager::with_core_path(root.join("nonexistent-csqtt"));
        {
            let mut rt = mgr.rt.write().await;
            rt.tunnel_ip = Some("10.0.0.2".parse().unwrap());
            rt.tun_attached = true;
            rt.tun_configured = true;
            rt.connected_streams = 0;
        }
        let state = test_state(&root, mgr, 1);
        let error = crate::api::execute_server_switch_workflow(&state, PROXY_NAME).await.unwrap_err();
        assert!(error.contains("Туннель ещё не подключился"), "unexpected error: {error}");
    }

    fn test_state(root: &std::path::Path, mgr: Arc<VkTurnManager>, mihomo_port: u16) -> crate::AppState {
        let yaml_path = root.join("config.yaml");
        let json_path = root.join("config.json");
        let mut cfg = crate::config::AppConfig::default();
        cfg.mihomo.config_path = yaml_path.display().to_string();
        cfg.mihomo.port = mihomo_port;
        let config_arc = Arc::new(tokio::sync::RwLock::new(Arc::new(cfg)));
        let ag = Arc::new(crate::antigravity::AntigravityManager::new(config_arc.clone()));
        crate::AppState {
            config: config_arc,
            config_path: Arc::new(json_path),
            http: reqwest::Client::new(),
            failover_log: Arc::new(crate::failover::FailoverLog::default()),
            routing_lock: Arc::new(tokio::sync::Mutex::new(())),
            config_lock: Arc::new(tokio::sync::Mutex::new(())),
            speedtest_lock: Arc::new(tokio::sync::Mutex::new(())),
            zapret_lock: Arc::new(tokio::sync::Mutex::new(())),
            antigravity: ag,
            vkturn: mgr,
        }
    }

    /// Мок Mihomo: на любой запрос отвечает 204 (reload «принят»).
    async fn spawn_mock_mihomo_204() -> u16 {
        use tokio::io::AsyncReadExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else { return };
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 16384];
                    let _ = sock.read(&mut buf).await;
                    let _ = sock.write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n").await;
                    let _ = sock.shutdown().await;
                });
            }
        });
        port
    }

    async fn response_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    const BASE_YAML: &str = "mode: rule\nproxies: []\nproxy-providers:\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies: [DIRECT]\nrules:\n  - MATCH,PROXY\n";

    async fn setup_saved_state(tag: &str) -> (std::path::PathBuf, crate::AppState) {
        let root = std::env::temp_dir().join(format!("xkeen-vkturn-{tag}-{}", crate::auth::random_hex(8)));
        std::fs::create_dir_all(&root).unwrap();
        let port = spawn_mock_mihomo_204().await;
        let state = test_state(&root, VkTurnManager::with_core_path(root.join("no-core")), port);
        std::fs::write(root.join("config.yaml"), BASE_YAML).unwrap();
        let cfg = (**state.config.read().await).clone();
        crate::config::save(&root.join("config.json"), &cfg).await.unwrap();
        (root, state)
    }

    fn enable_req() -> crate::api::SaveVkTurnConfigReq {
        crate::api::SaveVkTurnConfigReq {
            enabled: Some(true),
            peer: Some("1.2.3.4".into()),
            password: Some("s3cret-pass".into()),
            links: Some(vec!["https://vk.com/call/join/abcdEFGH1234".into()]),
            ..Default::default()
        }
    }

    /// Ревью: обработчик сам захватывал routing_lock/config_lock, а затем ConfigTx::begin
    /// захватывал их повторно — вечная взаимная блокировка.
    /// csqtt: провайдер (direct через csq0) добавляется сразу при включении и попадает в серверы БС.
    #[tokio::test]
    async fn regression_save_config_does_not_deadlock_and_adds_provider() {
        let (root, state) = setup_saved_state("save").await;
        let resp = tokio::time::timeout(
            Duration::from_secs(15),
            crate::api::save_vkturn_config(axum::extract::State(state.clone()), axum::Json(enable_req())),
        )
        .await
        .expect("сохранение VK-туннеля зависло (взаимная блокировка)");
        let v = response_json(resp).await;
        assert_eq!(v["success"], true, "ответ: {v}");
        assert!(state.routing_lock.try_lock().is_ok(), "routing_lock остался занятым");
        assert!(state.config_lock.try_lock().is_ok(), "config_lock остался занятым");

        let saved = state.config.read().await.vkturn.clone();
        assert_eq!(saved.peer, "1.2.3.4:46000", "PEER-порт по умолчанию подставлен");
        assert_eq!(saved.links, vec!["https://vk.com/call/join/abcdEFGH1234"]);

        let yaml = std::fs::read_to_string(root.join("config.yaml")).unwrap();
        assert!(yaml.contains(RULES_BEGIN), "правила DIRECT для TURN/VPS ставятся сразу");
        assert!(yaml.contains("  vkturn:"), "провайдер VK-туннеля добавлен: {yaml}");
        let prov = std::fs::read_to_string(root.join("providers").join("vkturn.yaml")).unwrap();
        assert!(prov.contains("interface-name: csq0"));
        assert!(prov.contains("routing-mark: 222"));
        assert!(state.config.read().await.whitelist_providers.iter().any(|p| p == PROVIDER_ID));

        // Повторное сохранение не теряет пароль
        let v = response_json(
            crate::api::save_vkturn_config(
                axum::extract::State(state.clone()),
                axum::Json(crate::api::SaveVkTurnConfigReq { streams: Some(30), ..Default::default() }),
            )
            .await,
        )
        .await;
        assert_eq!(v["success"], true, "ответ: {v}");
        assert_eq!(state.config.read().await.vkturn.password, "s3cret-pass", "пароль сохраняется");
        assert_eq!(state.config.read().await.vkturn.streams, 30);

        // Выключение через тот же обработчик тоже не зависает и снимает правила
        let resp = tokio::time::timeout(
            Duration::from_secs(15),
            crate::api::vkturn_action(
                axum::extract::State(state.clone()),
                axum::Json(crate::api::VkTurnActionReq { action: "disable".into() }),
            ),
        )
        .await
        .expect("выключение VK-туннеля зависло");
        assert_eq!(response_json(resp).await["success"], true);
        assert!(!std::fs::read_to_string(root.join("config.yaml")).unwrap().contains(RULES_BEGIN));

        // Сброс через reset очищает peer, password и links
        let reset_resp = tokio::time::timeout(
            Duration::from_secs(15),
            crate::api::vkturn_action(
                axum::extract::State(state.clone()),
                axum::Json(crate::api::VkTurnActionReq { action: "reset".into() }),
            ),
        )
        .await
        .expect("сброс VK-туннеля завис");
        let reset_json = response_json(reset_resp).await;
        assert_eq!(reset_json["success"], true);
        let cur = state.config.read().await.vkturn.clone();
        assert!(!cur.enabled);
        assert!(cur.peer.is_empty());
        assert!(cur.password.is_empty());
        assert!(cur.links.is_empty());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn regression_reset_clears_profile_token_and_runtime_state() {
        let (root, state) = setup_saved_state("reset-profile-token").await;
        let req = crate::api::SaveVkTurnConfigReq {
            enabled: Some(true),
            peer: Some("example.com:46000".into()),
            password: Some("dummy".into()),
            auth_mode: Some("profile_token".into()),
            profile_token: Some("profile-secret".into()),
            vk_token: Some("vk1.a.PersonalSavedToken".into()),
            streams: Some(35),
            ..Default::default()
        };
        let saved = response_json(crate::api::save_vkturn_config(axum::extract::State(state.clone()), axum::Json(req)).await).await;
        assert_eq!(saved["success"], true);
        {
            let mut rt = state.vkturn.rt.write().await;
            rt.phase = VkTurnPhase::AuthFailed;
            rt.last_error = Some("Сервер csqtt отклонил пароль".into());
            rt.error_source = Some("csqtt");
            rt.error_reason = Some("password_invalid");
            rt.restarts = 4;
            rt.bytes_up = 1024;
            rt.bytes_down = 2048;
            rt.log_tail.push_back("old log".into());
        }

        let reset_resp = crate::api::vkturn_action(
            axum::extract::State(state.clone()),
            axum::Json(crate::api::VkTurnActionReq { action: "reset".into() }),
        )
        .await;
        let reset_json = response_json(reset_resp).await;
        assert_eq!(reset_json["success"], true);
        assert!(!reset_json.to_string().contains("PersonalSavedToken"));

        let cur = state.config.read().await.vkturn.clone();
        assert!(!cur.enabled);
        assert!(cur.peer.is_empty());
        assert!(cur.password.is_empty());
        assert!(cur.links.is_empty());
        assert!(cur.profile_token.is_none());
        assert_eq!(cur.auth_mode, "oauth");
        assert_eq!(cur.vk_token.as_deref(), Some("vk1.a.PersonalSavedToken"));

        let rt = state.vkturn.rt.read().await;
        assert_eq!(rt.phase, VkTurnPhase::Disabled);
        assert!(rt.last_error.is_none());
        assert!(rt.error_source.is_none());
        assert!(rt.error_reason.is_none());
        assert_eq!(rt.restarts, 0);
        assert_eq!(rt.bytes_up, 0);
        assert_eq!(rt.bytes_down, 0);
        assert!(rt.log_tail.is_empty());
        drop(rt);

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn regression_logout_keeps_profile_source_and_running_status() {
        let (root, state) = setup_saved_state("profile-logout").await;
        let req = crate::api::SaveVkTurnConfigReq {
            enabled: Some(true), peer: Some("example.com:46000".into()), password: Some("dummy".into()),
            auth_mode: Some("profile_token".into()), profile_token: Some("profile-synthetic".into()),
            vk_token: Some("vk1.a.PersonalSyntheticToken".into()), ..Default::default()
        };
        let saved = response_json(crate::api::save_vkturn_config(axum::extract::State(state.clone()), axum::Json(req)).await).await;
        assert_eq!(saved["success"], true);
        state.vkturn.rt.write().await.phase = VkTurnPhase::Connected;
        let result = response_json(crate::api::logout_vkturn_auth(axum::extract::State(state.clone())).await).await;
        assert_eq!(result["success"], true);
        assert_eq!(state.vkturn.rt.read().await.phase, VkTurnPhase::Connected);
        let persisted = crate::config::load(&root.join("config.json"));
        assert_eq!(selected_vk_token(&persisted.vkturn), Some("profile-synthetic"));
        assert!(persisted.vkturn.vk_token.is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn regression_direct_profile_import_preserves_source_and_workers() {
        let (root, state) = setup_saved_state("direct-profile").await;
        let req = crate::api::SaveVkTurnConfigReq {
            enabled: Some(true),
            peer: Some("csqtt://connect?v=2&host=example.com&peer=46000&password=abc%2Bxyz&hashes=abcdefghijklmnop+qrstuvwxyzABCDEF&power=27".into()),
            vk_token: Some("vk1.a.PersonalSyntheticToken".into()),
            ..Default::default()
        };
        let result = response_json(crate::api::save_vkturn_config(axum::extract::State(state.clone()), axum::Json(req)).await).await;
        assert_eq!(result["success"], true);
        let saved = state.config.read().await.vkturn.clone();
        assert_eq!(saved.password, "abc+xyz");
        assert_eq!(saved.auth_mode, "manual");
        assert_eq!(call_hashes(&saved), vec!["abcdefghijklmnop", "qrstuvwxyzABCDEF"]);
        assert_eq!(saved.streams, 27);
        assert_eq!(selected_vk_token(&saved), None);
        let req = crate::api::SaveVkTurnConfigReq {
            peer: Some("csqtt://connect?v=2&host=second.example&peer=443&password=p&token=profile-synthetic&workers=35".into()),
            ..Default::default()
        };
        let result = response_json(crate::api::save_vkturn_config(axum::extract::State(state.clone()), axum::Json(req)).await).await;
        assert_eq!(result["success"], true);
        let saved = state.config.read().await.vkturn.clone();
        assert!(saved.links.is_empty());
        assert_eq!(selected_vk_token(&saved), Some("profile-synthetic"));
        assert_eq!(saved.vk_token.as_deref(), Some("vk1.a.PersonalSyntheticToken"));
        assert_eq!(saved.streams, 35);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn regression_profile_token_is_separate_and_masked() {
        let (root, state) = setup_saved_state("profile-token").await;
        let req: crate::api::SaveVkTurnConfigReq = serde_json::from_value(serde_json::json!({
            "enabled": true, "peer": "example.com:46000", "password": "dummy",
            "auth_mode": "profile_token", "profile_token": "profile-secret", "vk_token": "personal-secret", "streams": 27
        })).unwrap();
        let result = response_json(crate::api::save_vkturn_config(axum::extract::State(state.clone()), axum::Json(req)).await).await;
        assert_eq!(result["success"], true);
        let saved = state.config.read().await.vkturn.clone();
        assert_eq!(selected_vk_token(&saved), Some("profile-secret"));
        assert_eq!(saved.vk_token.as_deref(), Some("personal-secret"));
        assert_eq!(effective_workers(&saved), 27);
        let masked = crate::api::mask_settings_for_api(&**state.config.read().await);
        assert!(!masked.to_string().contains("profile-secret"));
        assert!(!masked.to_string().contains("personal-secret"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn regression_manual_source_requires_hashes_even_with_saved_account() {
        let mut source = cfg_with(&["abcdefghijklmnop"]);
        source.vk_token = Some("synthetic-token".into());
        source.auth_mode = "manual".into();
        assert_eq!(selected_vk_token(&source), None);
        assert_eq!(call_hashes(&source), vec!["abcdefghijklmnop"]);
        source.auth_mode = "oauth".into();
        assert_eq!(selected_vk_token(&source), Some("synthetic-token"));
        let (root, state) = setup_saved_state("manual-source").await;
        let req = crate::api::SaveVkTurnConfigReq {
            auth_mode: Some("manual".into()),
            vk_token: Some("synthetic-token".into()),
            links: Some(vec![]),
            ..enable_req()
        };
        let result = response_json(crate::api::save_vkturn_config(axum::extract::State(state.clone()), axum::Json(req)).await).await;
        assert_eq!(result["success"], false);
        assert!(!state.config.read().await.vkturn.enabled);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn regression_enable_requires_peer_password_and_vk_link() {
        let (root, state) = setup_saved_state("validate").await;
        for (req, what) in [
            (crate::api::SaveVkTurnConfigReq { peer: Some(String::new()), ..enable_req() }, "без адреса сервера"),
            (crate::api::SaveVkTurnConfigReq { password: Some(String::new()), ..enable_req() }, "без пароля"),
            (crate::api::SaveVkTurnConfigReq { links: Some(vec![]), ..enable_req() }, "без ссылки"),
            (
                crate::api::SaveVkTurnConfigReq {
                    links: Some(vec!["https://telemost.yandex.ru/j/12345678".into()]),
                    ..enable_req()
                },
                "со ссылкой Телемоста",
            ),
            (crate::api::SaveVkTurnConfigReq { password: Some("a\nb".into()), ..enable_req() }, "с переводом строки в пароле"),
        ] {
            let v = response_json(crate::api::save_vkturn_config(axum::extract::State(state.clone()), axum::Json(req)).await).await;
            assert_eq!(v["success"], false, "включение {what} должно отклоняться: {v}");
        }
        assert!(!state.config.read().await.vkturn.enabled);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Ревью: два одновременных restart() перезаписывали канал остановки — первый надзор «осиротевал»,
    /// и оказывались два клиента.
    #[tokio::test]
    async fn regression_concurrent_restarts_leave_single_supervisor() {
        let root = std::env::temp_dir().join(format!("xkeen-vkturn-race-{}", crate::auth::random_hex(8)));
        std::fs::create_dir_all(&root).unwrap();
        // Файл существует, но не исполняется: надзор жив и ждёт паузу после неудачного запуска.
        let fake_core = root.join("fake-core.txt");
        std::fs::write(&fake_core, "not a binary").unwrap();
        let mgr = VkTurnManager::with_core_path(&fake_core);
        let state = test_state(&root, mgr.clone(), 1);
        let cfg = cfg_with(&["https://vk.com/call/join/abcdEFGH1234"]);

        tokio::join!(
            mgr.restart(cfg.clone(), state.clone()),
            mgr.restart(cfg.clone(), state.clone()),
            mgr.restart(cfg.clone(), state.clone()),
        );
        assert_eq!(mgr.active_supervisors(), 1, "после одновременных перезапусков должен остаться один надзор");
        mgr.restart(cfg.clone(), state.clone()).await;
        assert_eq!(mgr.active_supervisors(), 1);
        mgr.stop().await;
        assert_eq!(mgr.active_supervisors(), 0, "stop() обязан дождаться завершения надзора");
        assert_eq!(mgr.rt.read().await.phase, VkTurnPhase::Disabled);

        // Без пароля клиент не запускается вовсе (csqtt сразу вышел бы с ошибкой)
        let mut no_pw = cfg.clone();
        no_pw.password.clear();
        mgr.restart(no_pw, state.clone()).await;
        assert_eq!(mgr.active_supervisors(), 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn regression_explanatory_phases_survive_backoff() {
        assert!(keeps_phase_during_backoff(VkTurnPhase::CaptchaRequired));
        assert!(keeps_phase_during_backoff(VkTurnPhase::LinkInvalid));
        assert!(keeps_phase_during_backoff(VkTurnPhase::AuthFailed));
        assert!(!keeps_phase_during_backoff(VkTurnPhase::Crashed));
        assert!(!keeps_phase_during_backoff(VkTurnPhase::Degraded));
        assert!(!keeps_phase_during_backoff(VkTurnPhase::Connected));
    }

    #[test]
    fn decide_after_exit_definitive_failures_pause_and_notify_once() {
        for phase in [VkTurnPhase::CaptchaRequired, VkTurnPhase::LinkInvalid, VkTurnPhase::AuthFailed] {
            let mut backoff = Duration::from_secs(5);
            let mut last = None;
            let d1 = decide_after_exit(phase, Duration::from_secs(10), &mut backoff, &mut last);
            assert_eq!(d1.wait, Duration::from_secs(600), "{phase:?}: не долбим ВК и сервер");
            assert!(d1.notice.is_some());
            let d2 = decide_after_exit(phase, Duration::from_secs(10), &mut backoff, &mut last);
            assert!(d2.notice.is_none(), "{phase:?}: повторное уведомление — спам");
            let d3 = decide_after_exit(phase, Duration::from_secs(400), &mut backoff, &mut last);
            assert!(d3.notice.is_some(), "{phase:?}: после долгой работы — новая серия");
        }
    }

    #[test]
    fn decide_after_exit_backoff_grows_and_resets() {
        let mut backoff = Duration::from_secs(5);
        let mut last = None;
        let d = decide_after_exit(VkTurnPhase::Crashed, Duration::from_secs(1), &mut backoff, &mut last);
        assert_eq!(d.wait, Duration::from_secs(10));
        for _ in 0..10 {
            decide_after_exit(VkTurnPhase::Crashed, Duration::from_secs(1), &mut backoff, &mut last);
        }
        assert_eq!(backoff, Duration::from_secs(120), "пауза ограничена 120 с");
        let d = decide_after_exit(VkTurnPhase::Crashed, Duration::from_secs(301), &mut backoff, &mut last);
        assert_eq!(d.wait, Duration::from_secs(5), "после долгой работы пауза сбрасывается");
    }

    /// Ревью: без исключений в iptables трафик клиента мог уйти в xkeen_mask (петля) или под nfqws.
    #[test]
    fn iptables_exemptions_cover_turn_and_peer() {
        let mut turn = BTreeSet::new();
        turn.insert("203.0.113.50".parse().unwrap());
        turn.insert("155.212.200.1".parse().unwrap()); // уже покрыт подсетью ВК
        let dsts = exempt_destinations(Some("198.51.100.7".parse().unwrap()), &turn);
        let mut expected = VK_NETWORK_CIDRS.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        expected.push("203.0.113.50/32".into());
        expected.push("198.51.100.7/32".into());
        assert_eq!(dsts, expected);
        assert_eq!(exempt_destinations(None, &BTreeSet::new()), VK_NETWORK_CIDRS);

        let apply = iptables_apply_script(&dsts);
        assert!(apply.contains("for ch in xkeen xkeen_mask XKEEN_BS zapret"));
        assert!(apply.contains("iptables -w -t"));
        assert!(!apply.contains("-w 2"));
        assert!(apply.contains("-m mark --mark 222"));
        assert!(apply.contains("-o csq0"));
        assert!(apply.contains("-i csq0"));
        assert!(apply.contains("-C \"$ch\""), "проверка -C делает правило идемпотентным");
        assert!(apply.contains("-I \"$ch\" 1"), "RETURN обязан стоять первым в цепочке");
        assert!(apply.contains(&format!("--comment {IPT_COMMENT} -j RETURN")));
        assert!(apply.contains("|| continue"), "отсутствующая цепочка пропускается");
        let remove = iptables_remove_script();
        assert!(remove.contains("for ch in xkeen xkeen_mask XKEEN_BS zapret"));
        assert!(remove.contains("iptables -w -t"));
        assert!(!remove.contains("-w 2"));
        assert!(remove.contains(&format!("--comment {IPT_COMMENT}")));
        assert!(remove.contains("sed 's/^-A /-D /'"));
    }

    #[tokio::test]
    async fn resolve_peer_ipv4_accepts_literal_and_rejects_garbage() {
        assert_eq!(resolve_peer_ipv4("1.2.3.4:46000").await, Some("1.2.3.4".parse().unwrap()));
        assert_eq!(resolve_peer_ipv4("no-port").await, None);
    }

    #[test]
    fn regression_narrow_direct_rules_and_router_exemptions() {
        let cfg = cfg_with(&["https://vk.com/call/join/abcdEFGH1234"]);
        let block = rules_block(&cfg);
        assert!(block.contains("IP-CIDR,155.212.192.0/20,DIRECT,no-resolve"));
        assert!(block.contains("IP-CIDR,1.2.3.4/32,DIRECT,no-resolve"));
        assert!(block.contains("DOMAIN,api.vk.com,DIRECT"));
        assert!(block.contains("DOMAIN,oauth.vk.ru,DIRECT"));
        assert!(!block.contains("DOMAIN,vk.com,DIRECT"));
        assert!(!block.contains("DOMAIN,vk.ru,DIRECT"));
        assert!(!block.contains("87.240.128.0/18"));
        assert!(!block.contains("95.213.0.0/17"));

        let mut turn = BTreeSet::new();
        turn.insert("87.240.130.10".parse().unwrap());
        turn.insert("95.213.10.20".parse().unwrap());
        turn.insert("203.0.113.50".parse().unwrap());
        let dsts = exempt_destinations(Some("198.51.100.7".parse().unwrap()), &turn);
        assert!(!dsts.contains(&"87.240.130.10/32".to_string()));
        assert!(!dsts.contains(&"95.213.10.20/32".to_string()));
        assert!(dsts.contains(&"203.0.113.50/32".to_string()));
        assert!(dsts.contains(&"198.51.100.7/32".to_string()));

        let apply = iptables_apply_script(&dsts);
        assert!(apply.contains("for ch in xkeen xkeen_mask XKEEN_BS zapret"));
        let remove = iptables_remove_script();
        assert!(remove.contains("for ch in xkeen xkeen_mask XKEEN_BS zapret"));
    }

    #[test]
    fn regression_vkturn_25_tun_configure_precleans_stale_csq0_and_supervise_tears_down() {
        let script = tun_configure_script("10.66.67.3".parse().unwrap());
        let check_pos = script
            .find("if printf '%s\\n' \"$existing_rules\"")
            .expect("foreign rule guard must stay");
        for stale_cleanup in [
            "ip rule del pref 222 dev csq0 table 222",
            "ip rule del pref 45 fwmark 222 table 222",
            "ip route del default dev csq0 table 222",
            "ip addr flush dev csq0",
        ] {
            let pos = script
                .find(stale_cleanup)
                .unwrap_or_else(|| panic!("missing pre-cleanup command: {stale_cleanup}"));
            assert!(
                pos < check_pos,
                "stale csq0 cleanup `{stale_cleanup}` must run BEFORE foreign rule check"
            );
        }
        assert!(
            script.contains("ip rule del pref 40 from") && script.find("ip rule del pref 40 from").unwrap() < check_pos,
            "stale pref 40 from old csq0 address must be pre-cleaned before foreign rule check"
        );

        let src = concat!(include_str!("auth.rs"), "\n", include_str!("tunnel.rs"), "\n", include_str!("health.rs"), "\n", include_str!("mod.rs"));
        let sup_body = src
            .split("async fn supervise(")
            .nth(1)
            .and_then(|s| s.split("fn spawn_log_reader(").next())
            .expect("supervise function must exist");
        assert!(
            sup_body.contains("tun_teardown_script_for_ip") && sup_body.contains("rt.tun_configured = false"),
            "supervise post-exit must tear down csq0 routes/rules and reset tun_configured before backoff"
        );

        let commit_body = src
            .split("pub async fn commit_vkturn")
            .nth(1)
            .and_then(|s| s.split("struct Supervisor").next())
            .expect("commit_vkturn function must exist");
        assert!(
            commit_body.contains("delete_provider_from_yaml"),
            "commit_vkturn must remove phantom vkturn provider when tunnel is disabled"
        );
    }

    #[test]
    fn regression_vkturn_speed_fix_mtu_mss_quic_and_mark222_exemptions() {
        // 1. MTU TUN = 900 / TCP MSS = 840 (учитывает оверхед WebRTC/TURN/WRAP-SRTP и пониженный Path MTU на LTE)
        assert_eq!(TUN_MTU, 900);
        assert_eq!(TUN_TCP_MSS, 840);

        // 2. TCP MSS clamping и мгновенный fallback с QUIC (UDP/443 REJECT) на csq0 + очистка
        let configure = tun_configure_script("10.66.67.3".parse().unwrap());
        let teardown = tun_teardown_script_for_ip(Some("10.66.67.3".parse().unwrap()));
        for ch in ["POSTROUTING", "OUTPUT"] {
            assert!(configure.contains(ch), "отсутствует цепочка исходящего зажима MSS: {ch}");
        }
        for ch in ["PREROUTING", "INPUT", "FORWARD"] {
            assert!(configure.contains(ch), "отсутствует цепочка входящего зажима MSS: {ch}");
        }
        assert!(configure.contains("-o csq0 -p tcp --tcp-flags SYN,RST SYN -m comment --comment xkeen-route-vkturn -j TCPMSS --set-mss 840"));
        assert!(configure.contains("-i csq0 -p tcp --tcp-flags SYN,RST SYN -m comment --comment xkeen-route-vkturn -j TCPMSS --set-mss 840"));
        assert!(configure.contains("-o csq0 -p udp --dport 443 -m comment --comment xkeen-route-vkturn -j REJECT"));

        assert!(teardown.contains("-o csq0 -p tcp --tcp-flags SYN,RST SYN -m comment --comment xkeen-route-vkturn -j TCPMSS --set-mss 840"));
        assert!(teardown.contains("-i csq0 -p tcp --tcp-flags SYN,RST SYN -m comment --comment xkeen-route-vkturn -j TCPMSS --set-mss 840"));
        assert!(teardown.contains("-o csq0 -p udp --dport 443 -m comment --comment xkeen-route-vkturn -j REJECT"));

        // 3. Исключения в xkeen/xkeen_mask/XKEEN_BS/zapret для mark 222 и интерфейса csq0 + полная очистка
        let apply = iptables_apply_script(&[]);
        assert!(apply.contains("-I \"$ch\" 1 -m mark --mark 222 -m comment --comment xkeen-route-vkturn -j RETURN"));
        assert!(apply.contains("-I \"$ch\" 1 -o csq0 -m comment --comment xkeen-route-vkturn -j RETURN"));
        assert!(apply.contains("-I \"$ch\" 1 -i csq0 -m comment --comment xkeen-route-vkturn -j RETURN"));

        let remove = iptables_remove_script();
        assert!(remove.contains("for ch in POSTROUTING OUTPUT FORWARD PREROUTING INPUT; do"));
        assert!(remove.contains("iptables -w -t mangle -S \"$ch\""));
        assert!(remove.contains("for ch in OUTPUT FORWARD; do"));
        assert!(remove.contains("iptables -w -t filter -S \"$ch\""));

        // 4. Провайдер vkturn использует IPv4-only и tfo: false (включая добавление и миграцию config.yaml)
        let prov_yaml = render_provider_yaml();
        assert!(prov_yaml.contains("ip-version: ipv4"));
        assert!(prov_yaml.contains("tfo: false"));
        assert!(!prov_yaml.contains("tfo: true"));

        let base = "proxy-providers:\n  other:\n    type: file\n    override:\n      tfo: true\nrules:\n  - MATCH,DIRECT\n";
        let with_vk = crate::routing::add_file_provider_to_yaml(base, PROVIDER_ID, "./providers/vkturn.yaml", None, None).unwrap();
        let vk_section = with_vk.split("  vkturn:").nth(1).unwrap().split("  other:").next().unwrap();
        assert!(vk_section.contains("tfo: false"), "add_file_provider_to_yaml обязан ставить tfo: false для vkturn: {vk_section}");
        assert!(!vk_section.contains("tfo: true"));
        let other_section = with_vk.split("  other:").nth(1).unwrap();
        assert!(other_section.contains("tfo: true"), "другие провайдеры не должны терять tfo: true");

        let legacy_yaml = "proxy-providers:\r\n  sub1:\r\n    type: http\r\n    override:\r\n      tfo: true\r\n  vkturn:\r\n    type: file\r\n    override:\r\n      udp: true\r\n      tfo: true\r\n  sub2:\r\n    type: file\r\n    override:\r\n      tfo: true\r\nrules:\r\n  - MATCH,PROXY\r\n";
        let normalized = normalize_vkturn_provider_in_yaml(legacy_yaml);
        assert!(normalized.contains("\r\n"), "CRLF должен сохраняться");
        let norm_vk = normalized.split("  vkturn:").nth(1).unwrap().split("  sub2:").next().unwrap();
        assert!(norm_vk.contains("tfo: false") && !norm_vk.contains("tfo: true"));
        assert!(normalized.split("  sub1:").nth(1).unwrap().split("  vkturn:").next().unwrap().contains("tfo: true"));
        assert!(normalized.split("  sub2:").nth(1).unwrap().contains("tfo: true"));
        assert_eq!(normalize_vkturn_provider_in_yaml(&normalized), normalized, "идемпотентность нормализации");
        let unchanged = "proxy-providers:\n  vkturn:\n    type: file\n    path: ./providers/vkturn.yaml\nrules:\n  - MATCH,PROXY\n";
        assert_eq!(normalize_vkturn_provider_in_yaml(unchanged), unchanged);
    }

    #[test]
    fn test_vkturn_provider_disables_udp_quic() {
        let yaml = render_provider_yaml();
        assert!(yaml.contains("udp: false"), "VK Calls provider must disable UDP/QUIC to force instant TCP: {yaml}");
    }

    #[test]
    fn test_iptables_apply_script_includes_filter_reject() {
        let script = iptables_apply_script(&[]);
        assert!(
            script.contains("-t filter -I \"$ch\" 1 -o csq0 -p udp --dport 443 -m comment --comment xkeen-route-vkturn -j REJECT --reject-with icmp-port-unreachable"),
            "iptables_apply_script must persistently include filter table UDP 443 REJECT: {script}"
        );
    }

    #[test]
    fn test_tun_configure_script_enables_mtu_probing() {
        let script = tun_configure_script("10.66.67.3".parse().unwrap());
        assert!(
            script.contains("echo 1 > /proc/sys/net/ipv4/tcp_mtu_probing"),
            "tun_configure_script must enable kernel tcp_mtu_probing: {script}"
        );
        assert!(
            script.contains("-t filter -I \"$ch\" 1 -o csq0 -p udp --dport 443 -m comment --comment xkeen-route-vkturn -j REJECT --reject-with icmp-port-unreachable"),
            "tun_configure_script must include icmp-port-unreachable reject: {script}"
        );
    }

    #[tokio::test]
    async fn regression_bug_a14_vkturn_notification_does_not_block() {
        let start = std::time::Instant::now();
        let handle = spawn_vkturn_notification_task(Duration::from_millis(50), async {
            tokio::time::sleep(Duration::from_secs(5)).await;
        });
        assert!(
            start.elapsed() < Duration::from_millis(40),
            "notification dispatch must not block caller"
        );
        let completed_within_timeout = handle.await.unwrap();
        assert!(
            !completed_within_timeout,
            "stalled notification task must time out instead of hanging"
        );
    }

    #[tokio::test]
    async fn regression_vkturn_commit_adds_provider_to_proxy_group_use() {
        let (root, state) = setup_saved_state("proxy-group-use").await;
        let resp = tokio::time::timeout(
            Duration::from_secs(15),
            crate::api::save_vkturn_config(axum::extract::State(state.clone()), axum::Json(enable_req())),
        )
        .await
        .expect("save_vkturn_config timed out");
        let v = response_json(resp).await;
        assert_eq!(v["success"], true, "unexpected response: {v}");
        let yaml = std::fs::read_to_string(root.join("config.yaml")).unwrap();
        assert!(
            yaml.contains("use:\n      - vkturn") || yaml.contains("- vkturn"),
            "PROXY group must include vkturn in use: {yaml}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
