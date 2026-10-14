# Результаты аудита Part 2 (Кодовая база XKeen Route)
> Зафиксировано из внешнего анализа нейросети для второй части бэкенда (api.rs после get_zapret_status, auth.rs, cdn_discovery.rs, config.rs, failover.rs, frontend.rs, logger.rs, main.rs, mihomo.rs до switch_flow_server).

## 🛡️ Раздел 1: Безопасность и уязвимости (Security & Vulnerabilities)

### [SEC-08] CSRF Guard Bypass при отсутствии заголовков Origin/Referer и доверие к чужим *.keenetic.pro
- **Category**: Security
- **Severity**: P1 - High
- **File & Location**: `backend/src/auth.rs` (`auth_middleware`, строки 427–452; `is_trusted_origin`, строки 383–394)
- **Проблема**: Если запрос не содержит заголовков `Origin` и `Referer`, проверка CSRF полностью пропускается (`if let Some(origin) = origin_header`). Кроме того, `is_trusted_origin` слепо доверяет любым поддоменам `*.keenetic.pro`, `*.keenetic.link`, `*.keenetic.name`, позволяя любому владельцу домена KeenDNS провести CSRF атаку на роутер жертвы.
- **Решение**: Убрать слепое доверие к `*.keenetic.pro/.link/.name` (доверять только если совпадает с `Host` запроса) и требовать валидный заголовок или кастомный заголовок SPA для мутирующих запросов.

### [SEC-09] Command Injection в iptables и sh -c через dev.ip в Gaming и Devices
- **Category**: Security
- **Severity**: P0 - Critical
- **File & Location**: `backend/src/api.rs` (`build_gaming_full_tunnel_commands`, строки 1042–1071; `toggle_device_zapret`, строки 822–880)
- **Проблема**: В `save_gaming_config` поле `dev.ip` десериализуется из клиентского JSON и без валидации вставляется в строку `iptables -s {ip} ...`, выполняемую через `sh -c`. Передача `ip = "192.168.1.5; reboot #"` приводит к выполнению произвольных shell-команд от имени root. В `toggle_device_zapret` `body.ip` и `body.mac` не проверяются перед добавлением в `excluded_devices`.
- **Решение**: Проверять `dev.ip` через `ip.parse::<std::net::Ipv4Addr>().is_ok()` и валидировать IP/MAC в `toggle_device_zapret`.

### [SEC-10] SSRF через do_sync_community_hostlist и scan_html_cdns
- **Category**: Security
- **Severity**: P2 - Medium
- **File & Location**: `backend/src/api.rs` (`do_sync_community_hostlist`) и `backend/src/cdn_discovery.rs` (`scan_html_cdns`)
- **Проблема**: В `do_sync_community_hostlist` параметр `url_override` принимается из запроса без проверки схемы/хоста (`http://127.0.0.1:79/rci/`), позволяя опрашивать внутренние сервисы роутера и LAN.
- **Решение**: Разрешать только схему `https://` и блокировать приватные/loopback IP-адреса.

### [SEC-11] Неограниченный рост памяти в FAILED_LOGIN_ATTEMPTS и сброс REVOKED_TOKENS
- **Category**: Security / Bug
- **Severity**: P2 - Medium
- **File & Location**: `backend/src/auth.rs` (`revoke_token`, `check_rate_limit`, `logout`)
- **Проблема**: `logout` вызывает `revoke_token` без проверки подлинности токена. Спам невалидными токенами доводит размер сета до > 10 000, вызывая `set.clear()`, что стирает все реально отозванные токены и реактивирует разлогиненные сессии. `FAILED_LOGIN_ATTEMPTS` не очищает старые IP без успешных входов.
- **Решение**: В `logout` вызывать `revoke_token` только после успешного `verify_session_token`, а в `check_rate_limit` очищать устаревшие IP.

---

## 🐛 Раздел 2: Баги, гонки и синхронизация состояний (Bug Hunting & State Desync)

### [BUG-09] Фиктивные замеры test_dpi и эмуляция mini_blockcheck
- **Category**: Bug / Logical Defect
- **Severity**: P1 - High
- **File & Location**: `backend/src/api.rs` (`zapret_action` -> `"test_dpi"`, `run_mini_blockcheck`)
- **Проблема**: В `test_dpi` для прямого канала Zapret возвращаются захардкоженные константы (`204` / `45ms`, `200` / `58ms`) без выполнения реальных запросов! В `run_mini_blockcheck` вместо тестирования стратегий к замеру прибавляется искусственный `jitter = idx * 0.005`, а баллы начисляются по наличию подстрок в аргументах, из-за чего автоподбор всегда выбирает одну и ту же стратегию (`aggressive_dupsid`). Трафик самого роутера идет мимо цепочки `PREROUTING`.
- **Решение**: Проводить реальные тесты через временный запуск nfqws или честно маркировать статическую эвристику.

### [BUG-10] Сброс правил xkeen-gaming-compat при перестройке netfilter в KeeneticOS
- **Category**: State Desync
- **Severity**: P1 - High
- **File & Location**: `backend/src/api.rs` (`build_gaming_full_tunnel_commands`) и `backend/src/main.rs`
- **Проблема**: Правила `xkeen-gaming-compat` добавляются однократно и сбрасываются при событиях DHCP/WAN/перезагрузке netfilter со стороны KeeneticOS, так как для них нет NDM netfilter-хука.
- **Решение**: Добавить восстановление игровых правил в NDM netfilter-хук (`/opt/etc/ndm/netfilter.d/`) и проверять их в вочдоге.

### [BUG-11] Вечная блокировка авторизации и риск затирания настроек при повреждении config.json
- **Category**: Bug / Data Loss
- **Severity**: P1 - High
- **File & Location**: `backend/src/config.rs` (`parse_config_content`, `load`)
- **Проблема**: При синтаксической ошибке в JSON возвращается `AppConfig::default()` с `auth.enabled = true`, но с пустыми `password_hash = ""` и `session_secret = ""`. Любой вход отклоняется (`expected_hash.is_empty() -> false`), блокируя доступ к панели до ручного сброса через SSH. Первое же фоновое сохранение перезапишет `config.json` дефолтным конфигом с заблокированным входом.
- **Решение**: Извлекать хэш пароля регулярным выражением из поврежденного файла и выставлять флаг readonly для поврежденного конфига.

### [BUG-12] Модификация config.json без state.config_lock в get_zapret_analytics
- **Category**: Bug / Concurrency
- **Severity**: P2 - Medium
- **File & Location**: `backend/src/api.rs` (`get_zapret_analytics`)
- **Проблема**: Функция модифицирует `state.config` и пишет на диск без захвата `state.config_lock`, создавая риск перезаписи параллельных транзакций `ConfigTx` устаревшим состоянием.
- **Решение**: Хранить счетчик в `AtomicU64` в памяти, сохранять через `try_lock()`.

### [BUG-13] Ложное срабатывание детектора IP-групп на внешние DST-IP-CIDR
- **Category**: Bug
- **Severity**: P2 - Medium
- **File & Location**: `backend/src/mihomo.rs` (`ip_groups_from_rules`)
- **Проблема**: Условие `|| payload.contains('/')` ловит правила `IP-CIDR,8.8.8.0/24,DIRECT` и заносит внешние подсети в карту устройств LAN.
- **Решение**: Проверять только `ptype.contains("SRC-IP")`.

### [BUG-14] Отказ сохранения настроек AdGuard Home при остановленном Mihomo
- **Category**: Bug
- **Severity**: P2 - Medium
- **File & Location**: `backend/src/api.rs` (`adguard_set_config`)
- **Проблема**: Вызывается `commit_and_reload()` вместо `commit_without_reload()`. Ошибка перезагрузки незапущенного Mihomo откатывает сохранение настроек AdGuard.
- **Решение**: Использовать `commit_without_reload()`.

---

## 🔍 Раздел 3: Хардкод (Hardcode Detection — Part 2)

### [HC-05] Хардкод прокси http://127.0.0.1:7890 в shell-скриптах zapret_action
- В `upgrade_zapret2`, `rollback_v1`, `test_dpi` зашит `-x http://127.0.0.1:7890`, ломая работу при смене `mixed_port`.

### [HC-06] Хардкод только 192.168.0.0/16 в Gaming Compatibility
- В `build_gaming_full_tunnel_commands` исключается только `192.168.0.0/16`, заворачивая трафик локальных сетей `10.0.0.0/8` и `172.16.0.0/12` в REDIRECT/TPROXY.

### [HC-07] Хардкод параметров в config.rs и main.rs
- В `AntigravityConfig::default()` зашиты `wan_interface: "apcli1"`, `vpn_interface: "nwg0"`.
- В `create_init` шаблонизируются жесткие пути и `crontab -`, игнорируя флаг `-c` и файл `/opt/etc/crontab`.
- Fallback порты `5000` и `5001` в парсинге redir/tproxy.
