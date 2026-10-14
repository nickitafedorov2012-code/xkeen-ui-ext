# Результаты аудита Part 3 (Кодовая база XKeen Route)
> Зафиксировано из внешнего анализа нейросети для третьей части бэкенда (mihomo.rs с switch_flow_server, override_sync.rs, rci.rs, routing.rs, transaction.rs, notifications.rs, speedtest.rs, system.rs, traffic.rs, updater.rs, watchdog.rs до spawn_dhcp_device_monitor).

## 🛡️ Раздел 1: Безопасность и уязвимости (Security & Vulnerabilities)

### [SEC-12] YAML Injection через add_provider_to_yaml_full и apply_flow_rules
- **Category**: Security
- **Severity**: P1 - High
- **File & Location**: `backend/src/routing.rs` (`add_provider_to_yaml_full`, строки 192–245; `apply_flow_rules`, строки 574–598)
- **Проблема**: В `add_provider_to_yaml_full` параметры `url`, `hwid`, `user_agent` подставляются напрямую в шаблон YAML без экранирования кавычек `"` и переносов строк `\n`. Злоумышленник может внедрить произвольные директивы (например, `path:` для перезаписи файлов роутера содержимым внешней подписки). В `apply_flow_rules` параметр `target` не проверяется на переносы строк перед вставкой в правило.
- **Решение**: Запретить символы `\r`, `\n`, `"`, `'`, `\` в `url`, `hwid`, `user_agent` и `target`, валидировать ID подписки.

### [SEC-13] SSRF и подмена бинарника ядра Mihomo через custom_url в POST /api/mihomo/install
- **Category**: Security
- **Severity**: P1 - High
- **File & Location**: `backend/src/updater.rs` (`mihomo_install`, строки 427–465)
- **Проблема**: Параметр `custom_url` позволяет скачать архив с любого произвольного сервера, распаковать его и запустить бинарник от имени `root` (`Command::new(&bin_path).arg("-v")`) для валидации.
- **Решение**: Ограничить `custom_url` только доверенными доменами (`https://github.com/MetaCubeX/mihomo/releases/download/`) или собирать URL строго из `tag`.

### [SEC-14] HTML-инъекция в notifications::send_telegram и SSRF в send_webhook
- **Category**: Security / Bug
- **Severity**: P2 - Medium
- **File & Location**: `backend/src/notifications.rs` (`send_telegram`, `notify_failover`, `send_webhook`)
- **Проблема**: В `send_telegram` включен режим `HTML`, но имя сервера и текст ошибки не экранируют `<`, `>`, `&`. Символы `<`/`>` в имени прокси ломают парсинг Telegram Bot API (ошибка 400), из-за чего критические алерты не доставляются. В `send_webhook` URL не проверяется на приватные IP и loopback.
- **Решение**: Экранировать HTML-спецсимволы в именах серверов (`escape_html`) и валидировать webhook URL.

---

## 🐛 Раздел 2: Баги, гонки и транзакционные дефекты (Bug Hunting & Concurrency)

### [BUG-15] Нарушение иерархии блокировок и гонка в switch_flow_server и watchdog
- **Category**: Bug / Concurrency / Routing / Tx Flaw
- **Severity**: P0 - Critical
- **File & Location**: `backend/src/mihomo.rs` (`switch_flow_server`) и `backend/src/watchdog.rs` (`spawn`)
- **Проблема**: `switch_flow_server` читает и пишет `config.yaml` напрямую без захвата `config_lock`/`routing_lock` и без транзакции `ConfigTx`. В `watchdog.rs` захватывается только `routing_lock` без предварительного `config_lock`, нарушая строгий порядок блокировок `config_lock -> routing_lock` и создавая риск коллизий транзакций.
- **Решение**: В `watchdog.rs` брать `config_lock` перед `routing_lock`, а запись в `switch_flow_server` выполнять под защитой блокировок.

### [BUG-16] Бесконечный цикл перезаписи config.yaml в watchdog каждые 4 секунды
- **Category**: Bug / State Desync
- **Severity**: P0 - Critical
- **File & Location**: `backend/src/watchdog.rs` (`spawn`, строки 115–195)
- **Проблема**: Если в `force_domains` есть только домены, перехваченные Zapret (`is_zapret_direct_domain`), генератор `routing::apply_routing` не создает блок `AUTO-FORCE`. Но `watchdog` проверяет сырой `!force_domains.is_empty()`, видит отсутствие `AUTO-FORCE` (`missing_force = true`) и перезаписывает `config.yaml`, вызывая бесконечный перезапуск Mihomo и износ флеш-памяти роутера.
- **Решение**: В `watchdog.rs` проверять наличие эффективных `force_domains` с учетом фильтрации Zapret Direct, а для устройств проверять непустоту списка серверов.

### [BUG-17] Поломка правил Mihomo при имени группы прокси, отличном от PROXY
- **Category**: Bug / Routing / Tx Flaw
- **Severity**: P1 - High
- **File & Location**: `backend/src/routing.rs` (`apply_domain_rules`, строка 792)
- **Проблема**: Для всех `force_domains` целевая группа захардкожена как `PROXY`. Если в шаблоне конфига группа называется `Proxy` или `GLOBAL`, Mihomo отвергает конфиг с ошибкой `Target group not found: PROXY`.
- **Решение**: Использовать `find_proxy_target_group(&content)` динамически.

### [BUG-18] Инвертированный приоритет правил в routing::apply_routing (LIFO вставка)
- **Category**: Bug / Routing
- **Severity**: P2 - Medium
- **File & Location**: `backend/src/routing.rs` (`apply_routing`, строки 997–1070)
- **Проблема**: Все блоки правил вставляются сразу после строки `rules:`. Из-за LIFO порядка общие правила `SRC-IP-CIDR` (`device_routing` и `Gaming`) оказываются ВЫШЕ, чем `DEV_DOMAINS` (персональные доменные правила устройства), `DIRECT`, `Google AI` и `AdBlock`. В Mihomo общее правило `SRC-IP-CIDR` перехватывает 100% трафика устройства раньше доменных проверок, полностью отключая AdBlock, прямой обход и персональные домены для устройств с назначенным сервером.
- **Решение**: Вызывать вставки от общего к частному: сначала `SRC-IP-CIDR` (устройства), затем персональные домены, Zapret direct, Google AI, и последним — `AdBlock` (чтобы оказаться самым верхним правилом).

### [BUG-19] Потеря мутированного словаря provider_filters в apply_routing
- **Category**: Bug / State Desync
- **Severity**: P2 - Medium
- **File & Location**: `backend/src/routing.rs` (`apply_routing`, строки 1063–1064)
- **Проблема**: Локальная копия `filters` модифицируется в `apply_ignore_to_providers`, но не сохраняется обратно в `cfg.provider_filters`.
- **Решение**: Сохранять мутированные фильтры при успешном коммите.

### [BUG-20] Некорректный расчет CPU% в rci::get_proc_stats и api::get_zapret_analytics
- **Category**: Bug
- **Severity**: P3 - Minor
- **File & Location**: `backend/src/rci.rs` (`get_proc_stats`) и `backend/src/api.rs` (`get_zapret_analytics`)
- **Проблема**: Расчет `app_cpu_percent` не делится на число ядер и может превышать 100%. В `get_zapret_analytics` переменная `nfqws_cpu_pct` захардкожена в `0.0`.
- **Решение**: Нормировать CPU% на число ядер и вычислять реальную нагрузку nfqws.

---

## 🔍 Раздел 3: Хардкод (Hardcode Detection — Part 3)

### [HC-08] Захардкоженные параметры в updater, rci, traffic, speedtest, routing
- В `updater.rs`: репозиторий `nickitafedorov2012-code/xkeen-ui-ext`, зеркала jsDelivr/ghproxy, прямой вызов `/opt/sbin/mihomo -v` вместо `cfg.system.mihomo_bin`.
- В `rci.rs`: DNS-серверы `["77.88.8.8", "77.88.8.1", "1.1.1.1"]`, путь `/opt/etc/xkeen/xkeen.json`.
- В `override_sync.rs`: пути `ru_exclude_override.lst`, `xkeen.conf`, `/opt/etc/crontab`, ipset `geo_override`.
- В `traffic.rs`: fallback WAN-интерфейс `apcli1`, префиксы `apcli`, `wwan`, `eth3`, `ppp`, `qmi_br`.
- В `speedtest.rs`: Cloudflare/Cachefly URL для скачивания тестов скорости.
- В `routing.rs`: User-Agent по умолчанию `"ClashMeta/1.19.24; mihomo/1.19.24"`, интервал обновления 86400с.
