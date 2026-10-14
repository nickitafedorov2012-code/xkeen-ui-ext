# Результаты аудита Part 1 (Кодовая база XKeen Route)
> Зафиксировано из внешнего анализа нейросети для первой половины кодовой базы (Cargo.toml, adguard.rs, antigravity.rs, api.rs до get_zapret_status).

## 🛡️ Раздел 1: Безопасность и уязвимости (Security & Vulnerabilities)

### [SEC-01] RCE (Command Injection) и обход фильтрации через `custom_entries` и `excluded_devices` в генераторе `zapret.conf`
- **Category**: Security
- **Severity**: P0 - Critical
- **File & Location**: `backend/src/api.rs` (`sync_zapret_files`, `S51ZAPRET_SCRIPT`)
- **Проблема**: В `sync_zapret_files` устройства из `cfg.excluded_devices` сериализуются без строгой проверки IP/MAC (простой `.filter(|d| !d.contains(':'))`). Внутри `S51ZAPRET_SCRIPT` переменные подставляются в `iptables -t mangle -A zapret -s "$ex_ip"`. Злоумышленник или поврежденный конфиг могут передать перенос строки или аргументы `iptables` (`-j ACCEPT`, `--lua-init` в nfqws2).
- **Решение**: Валидировать IP/CIDR через `crate::config::is_valid_ip_or_cidr` и MAC-адреса через regex/hex-парсер.

### [SEC-02] Инъекция команд CLI Keenetic (`ndmc -c`) и `iptables` через невалидированный `antigravity.targets`
- **Category**: Security
- **Severity**: P0 - Critical
- **File & Location**: `backend/src/antigravity.rs` (`apply_keenetic_rules`, `clean_keenetic_rules`)
- **Проблема**: Массив `targets` напрямую интерполируется в строку `ndmc -c "ip host {target} {ip}"`.
- **Решение**: Проверять домены через `crate::config::is_valid_domain(target)`.

### [SEC-03] Path Traversal в `delete_provider`, `add_provider` и `import_node`
- **Category**: Security
- **Severity**: P0 - Critical
- **File & Location**: `backend/src/api.rs` (`delete_provider`, `add_provider`, `import_node`)
- **Проблема**: В `delete_provider` параметр `req.id` не проверяется на `..`, формируя `/opt/etc/mihomo/providers/{id}.yaml`. В `import_node` параметр `provider_name` без валидации передается в `.join(format!("{}.yaml", prov_name))`.
- **Решение**: Ввести строгий валидатор `is_valid_provider_id` (`[a-zA-Z0-9_-]`, длина <= 64).

### [SEC-04] SSRF во внутреннюю сеть роутера и LAN через HTTP CONNECT прокси Antigravity
- **Category**: Security
- **Severity**: P1 - High
- **File & Location**: `backend/src/antigravity.rs` (`handle_proxy_conn`, строки 485–555)
- **Проблема**: Прокси позволяет клиентам коннектиться к любому `target_host_port`, даже не входящему в `targets`, включая локальные порты 127.0.0.1:9090 (Mihomo API без секрета) и 127.0.0.1:79.
- **Решение**: Разрешать CONNECT только для доменов из `antigravity.targets` и строго на 443 порт, запретив доступ к loopback/RFC1918.

### [SEC-05] Обход фильтрации `init_script` и Command Injection в `xkeen_service` и `adguard::service_action`
- **Category**: Security
- **Severity**: P1 - High
- **File & Location**: `backend/src/api.rs` (`xkeen_service`) и `backend/src/adguard.rs` (`service_action`)
- **Проблема**: В `init_script` проверяются спецсимволы по черному списку, но пропущены двойные кавычки `"` и перенос строки `\n`. В `adguard.rs` имя скрипта оборачивается в одинарные кавычки.
- **Решение**: Белый список пути `init_script.starts_with("/opt/etc/init.d/")` и проверка допустимых символов.

### [SEC-06] Загрузка бинарников AdGuard Home с отключенной проверкой TLS (`curl -k` / `wget --no-check-certificate`)
- **Category**: Security
- **Severity**: P1 - High
- **File & Location**: `backend/src/adguard.rs` (`install_adguard_package`)
- **Проблема**: Скачивание архива с `ghproxy.net` без проверки сертификатов и без SHA256-хэша.
- **Решение**: Убрать `-k` и проверять контрольные суммы или скачивать через `reqwest` с `rustls`.

### [SEC-07] Утечка секретов в открытом виде и риск перезаписи маской `"******"`
- **Category**: Security
- **Severity**: P1 - High
- **File & Location**: `backend/src/api.rs` (`get_settings`, `read_config_file`, `export_backup`)
- **Проблема**: В `get_settings` забыто маскирование `adguard.password`. В `read_config_file?file=route` сырой `config.json` отдается без маскирования. При `put_settings` есть риск перезаписи пароля строкой `"******"`.
- **Решение**: Маскировать `adguard.password` и игнорировать `"******"` при сохранении.

---

## 🐛 Раздел 2: Баги, гонки и синхронизация состояний (Bug Hunting & State Desync)

### [BUG-01] Накопление дубликатов `ip rule` и баг парсера DNS сжатых указателей в `antigravity.rs`
- **Category**: Bug / State Desync
- **Severity**: P0 - Critical
- **File & Location**: `backend/src/antigravity.rs` (`apply_keenetic_rules`, `parse_dns_a_records`)
- **Проблема**: `ip rule add` не дедуплицирует правила; в `parse_dns_a_records` байт `0xC0` в Answer section читается как длина метки 192, вызывая прыжок за пределы пакета и потерю A-записей.
- **Решение**: Проверять `(b & 0xC0) == 0xC0` и удалять старые копии `ip rule` перед добавлением.

### [BUG-02] Однократный `SHUTDOWN` флаг (`AtomicBool`) навсегда блокирует перезапуск Antigravity
- **Category**: Bug / Concurrency
- **Severity**: P1 - High
- **File & Location**: `backend/src/antigravity.rs` (`SHUTDOWN`, `start_proxy`)
- **Проблема**: Флаг никогда не сбрасывается в `false`, а смена порта прокси не останавливает старый листенер.
- **Решение**: Использовать `tokio::sync::watch` или `AbortHandle`.

### [BUG-03] Гонка записи `config.yaml` в `add_provider`/`delete_provider` в обход `ConfigTx`
- **Category**: State Desync / Concurrency
- **Severity**: P1 - High
- **File & Location**: `backend/src/api.rs` (`add_provider`, `delete_provider`, `set_ignore`, `fix_names`)
- **Проблема**: Модификация `config.yaml` без захвата `routing_lock` / `ConfigTx`.
- **Решение**: Использовать `ConfigTx::begin(&state).await`.

### [BUG-04] Ошибки модификации `config.yaml` в `import_node`
- **Category**: Bug
- **Severity**: P1 - High
- **File & Location**: `backend/src/api.rs` (`import_node`)
- **Проблема**: При `proxies: []` создается второй ключ `proxies:`, а проверка провайдера `contains(&format!("{}:", prov_name))` ложно срабатывает на стандартные YAML-ключи.
- **Решение**: Использовать `routing::parse_provider_names` и регулярное выражение для секции `proxies`.

### [BUG-05] Блокирующие синхронные вызовы `std::process::Command`, `std::fs`, `std::net` в Tokio
- **Category**: Bug / Concurrency
- **Severity**: P2 - Medium
- **File & Location**: `backend/src/adguard.rs` и `backend/src/api.rs` (`ws_logs_stream`)
- **Проблема**: Синхронный `iptables`, `which`, `UdpSocket::bind`, `logger::tail` без `spawn_blocking`.
- **Решение**: Использовать `tokio::process::Command` и `spawn_blocking`.

### [BUG-06] Несовпадение синтаксиса роутов Axum 0.8 и лимит 2 МБ на JSON
- **Category**: Bug
- **Severity**: P2 - Medium
- **File & Location**: `backend/Cargo.toml` и `backend/src/api.rs`
- **Проблема**: Axum 0.8 требует `{param}` вместо `:param`. Лимит `axum::extract::Json` 2 МБ блокирует импорт `.xkbak`.
- **Решение**: Убедиться в правильном синтаксисе роутов и добавить `DefaultBodyLimit::max(10 * 1024 * 1024)`.

### [BUG-07] Деструктивный side-effect в `GET /api/zapret/status`
- **Category**: Bug / State Desync
- **Severity**: P2 - Medium
- **File & Location**: `backend/src/api.rs` (`get_zapret_status`)
- **Проблема**: GET-запрос статуса убивает nfqws2 и вызывает `S51zapret stop-fw`, если в конфиге `zapret.enabled = false`.
- **Решение**: Убрать убийство процессов из GET-хендлера, перенести в фоновый вочдог.

### [BUG-08] Пропуск `AdGuardHome.yaml` в `export_backup` и блокировка при импорте
- **Category**: State Desync
- **Severity**: P2 - Medium
- **File & Location**: `backend/src/api.rs` (`export_backup`, `import_backup`)
- **Проблема**: `AdGuardHome.yaml` не экспортируется в архив `.xkbak` и отклоняется при импорте.
- **Решение**: Добавить в белый список архива бэкапа.

---

## 🔍 Раздел 3: Хардкод (Hardcode Detection)

### [HC-01] Захардкоженные IP, DNS и SmartDNS
- `192.168.1.1` (fallback IP), `127.0.0.1:1053`, `8.8.8.8:53`.
- SmartDNS: `83.220.169.155`, `212.109.195.93`, `45.155.204.190`, `192.144.59.14`, `94.232.43.149`, `111.88.96.54`, `111.88.96.50`.
- Xbox-DNS: `111.88.96.54`, `111.88.96.55`, `188.68.214.130`, `188.68.214.143`.
- Пинг `77.88.8.8` и `8.8.8.8` в S51zapret.

### [HC-02] Захардкоженные порты, fwmark, таблицы и qnum
- Порты `53`, `5353`, `3000`. Автосмена порта AdGuard с 53 на 5353 не обновляет `AppConfig`.
- `priority 90`, таблица `main`, ipset `user_exclude`.
- Hex-порт `:041D` (1053), `fwmark 0x111/0xff`, qnum 200, fwmark `0x40000000`.

### [HC-03] Оператор `||` для интерфейсов `br+` и `Bridge+` в `S51ZAPRET_SCRIPT`
- В `S51ZAPRET_SCRIPT` команда `iptables -i br+ ... || iptables -i Bridge+ ...` никогда не доходит до `Bridge+`, если правило `br+` добавилось успешно.

### [HC-04] Захардкоженные версии, URL и эвристики серверов
- Версии `v72.13` и `v1.0.5.2` при отсутствии `version.txt`.
- Внешние URL `static.adguard.com`, `ghproxy.net`, `meta-rules-dat`, `cp.cloudflare.com`.
- Тройное дублирование поиска США/Канады по строкам `сша`, `usa`, `us `, `chicago` без поддержки эмодзи флагов `🇺🇸`, `🇨🇦` и штатов.
