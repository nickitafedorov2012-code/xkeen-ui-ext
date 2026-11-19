# Каталог дублирования функций и логики (`D1–D16`)

Настоящий каталог фиксирует все подтверждённые кластеры дублирования функций на бэкенде (`Rust` + shell-генераторы) и фронтенде (`TypeScript/React` + `CSS`), ответ на вопрос пользователя «почему функции друг друга дублируют» и конкретные расхождения поведения между копиями.

---

## 1. Почему в кодовой базе возникли массовые дубли

1. **Изолированные фиксы под конкретные тикеты:** при добавлении новой вкладки или режима (VK-tunnel, FreeSub пул, Google AI Flow, Smart Gaming, Watchdog) автор копировал ближайший хелпер (`ping_*`, `switch_*`, `formatBytes`, `remove_*_rules`), слегка меняя параметры, вместо расширения общего модуля.
2. **Обходной дубляж вокруг виртуальных карт (`free_wl`/`free_vpn`):** вместо унификации модели пула каждая подсистема дописывала собственные фильтры, часть из которых забывала проверять `free_wl`/`free_vpn`.
3. **Разрыв между ручными строковыми парсерами YAML/CLI:** каждый разработчик писал свой локальный парсер куска YAML, `/proc/net/arp`, `df`, `base64` или URL-кодировщика.

---

## 2. Кластеры дублирования на Backend (Rust / Shell)

### D1. Переключение активного сервера и групп (`7` точек входа — `[B2]` / `[B3]`)
| Функция / точка входа | Место | Обновление `wl`/БС | Инвалидация кэша / сброс сокетов |
|---|---|---|---|
| `switch_server` / `switch_server_inner` | `mihomo.rs:1641-1831` | В `HEAD`: только при `whitelist.enabled`; в WIP diff: всегда при наличии в `🛡️ Whitelist` | Сбрасывает сокеты только при уходе с `DIRECT` (`1798-1802`) |
| `switch_group_scoped` | `mihomo.rs:2929` | Нет (точечная группа) | Закрывает сокеты группы |
| `switch_selector` | `mihomo.rs:2937` | Нет | Не сбрасывает |
| `switch_gaming_group` | `mihomo.rs:359` | Проверяется вызывающим кодом | Точечно в группе |
| `switch_flow_server` | `mihomo.rs:1869-1910` | Нет (`Google AI`) | Игнорирует ошибку `m_put` (`1908`) |
| `free_sub::switch_pool_node` | `free_sub/mod.rs:2349-2435` | Вызывает `switch_server` **до** `sync_mode_whitelist_state` (сломано на `beta.11.0`) | Не откатывается при ошибке |
| `execute_server_switch_workflow` | `api/servers/mod.rs:351-380` | Вызывает `free_sub` или `mihomo` | Молча глотает ошибку `switch_pool_node` (`361-370`) |

### D2. Ping / L7-пробинг (`6`+ параллельных реализаций — `[B2]` / `[B5]`)
- `mihomo.rs:1928` (`ping_group_url`)
- `mihomo.rs:2003` (`ping_server_url_expected_range`) — требует `expected=204` по умолчанию, ломая custom URL с `200`
- `mihomo.rs:2237` (`probe_server_l7`) — напрямую переключает общий селектор `Canary Probe` без `speedtest_lock`!
- `mihomo.rs:2361` (`ping_server_l7_split`: `PRIMARY_L7_CHECK_URL = http://cp.cloudflare.com/...` + `SECONDARY_L7_CHECK_URL = https://www.gstatic.com/...`)
- `mihomo.rs:923, 950` (`probe_service_provider_node_l7` и `_stable`: порядок URL наоборот — `gstatic` -> `cp.cloudflare`, бюджет без общего лимита)
- `mihomo.rs:2422` (`ping_all_url`) — единственная массовая функция, обновляющая `PING_CACHE` (в паре с пост-свитч задачей)
- `free_sub/mod.rs:895-908` (`verify_node_stability`) — мёртвый дубль стабильной L7-проверки.

### D3. Распознавание БС-групп и узлов БС (`3+` расходящихся детектора)
- `whitelist_tags.rs:5-85` — канонические маркеры
- `mihomo.rs:1671-1673` (`select_groups_to_switch`) — проверяет `WHITELIST_GROUP_NAME | contains("whitelist") | contains("белые списки")` (lowercase)
- `mihomo.rs:2660-2663` (`evaluate_selector_auto_heal_for_group`) — проверяет `WHITELIST_GROUP_NAME | starts_with("🛡️ ") | starts_with("📱 ") | contains("Whitelist")` (с заглавной W!) и зовёт `is_whitelist_server_full(..., None, cfg)` без имени провайдера
- `failover/probe.rs:542-568` — забывает исключить виртуальные `free_vpn`/`free_wl` с `is_pool = false`.

### D4. Детектор группы `Google AI` / исключений Flow (`[B2-009]`)
- `is_flow_group_name` vs `api/servers/mod.rs:649-682` (`is_excluded_for_flow`, пропускает только `AUTO-`, забывая `{Имя} 192_168_...` и `DEV_...`) vs ручные обходы в `find_proxy_target_group` (`routing/dns_redirect.rs:716`, где `Google AI` ошибочно стоит раньше `Fastest`/`Auto`/`Fallback`!).

### D5. Атомарная запись файлов (`6` реализаций с разной надёжностью — `[B1]`, `[B3]`, `[B4]`)
1. `config.rs:1702` (`config::save`: async + `.tmp` + `sync_all()` + `.bak` + синхронный `dir.sync_all()`)
2. `config.rs:1601` (`write_file_synced`: sync + `sync_all()`)
3. `api/mod.rs:60` (`atomic_write_file`: async + `sync_all()`, без `dir.sync_all()`)
4. `transaction.rs:52` (`sync_atomic_write_or_remove`: sync **без `sync_all()`**)
5. `auth.rs:110, 917, 939` (`persist_revoked_tokens_to` с `sync_all()`, но `cli_*_password` — **без `sync_all()`** и с `with_extension("tmp")`)
6. `override_sync.rs:166, 399`, `failover/probe.rs:424-435`, `watchdog.rs:384`, `traffic.rs:286` (последние два используют `.with_extension("tmp")`, затирая `.json`, и не делают `fsync`; `adguard/yaml.rs:645` вообще пишет через голый `std::fs::write`).

### D6. Парсинг подписок и редактирование YAML-блоков
- `subscription_parser.rs:1159` (`parse_subscription_nodes_bounded` — с лимитом `MAX_PARSED_NODES`, запретом `127.0.0.1`/LAN, проверкой порта и уникальностью имён) **vs** `subscription_parser.rs:1277` (`convert_subscription_to_mihomo_yaml_with_report` — используется в `add_provider` и `refresh_mobile_sub`, **без** всех этих проверок, `[B2-007]`).
- `free_sub/mod.rs:1036-1138` (`parse_yaml_block`) **vs** строковые манипуляции `routing/validator.rs` **vs** сырая вставка в `api/servers/mod.rs:1248-1308` (`import_node`) **vs** глобальный `String::replace` по всему файлу в `api/routing/devices.rs:202-227` (`fix_names`).
- **10 копий удаления маркерных блоков `# BEGIN ... # END`:** `remove_flow_rules`, `remove_adblock_rules`, `remove_zapret_hybrid_rules`, `remove_domain_blocks`, `remove_quic_reject_rules`, `remove_whitelist_rules`, `remove_whitelist_dns`, `remove_gaming_rules`, `remove_canary_rules`, `vkturn::remove_rules` — с расходящейся обработкой `\r\n` и незакрытого END-маркера.

### D7. MAC → IP разрешение (`/proc/net/arp`) и списки исключений устройств (`[B4]`, `[B6]`)
- `routing/ipset.rs`: `find_ip_for_mac_in_arp`, `resolve_excluded_whitelist_ips_internal`, `resolve_excluded_zapret_ips_internal` — трижды повторяют цикл чтения `/proc/net/arp`.
- При этом `apply_gaming_rules_with_zapret` (`routing/dns_redirect.rs:1287, 1335`) этот резолв вообще не вызывает, молча отбрасывая MAC-only устройства.

### D8. Системные утилиты (при наличии готовых крейтов в `Cargo.toml`) и метрики `/proc`
- **Вывод `df` (`3` копии):** `system.rs:1333` (корректно склеивает перенесённые строки BusyBox) **vs** `updater.rs:1049-1071` **vs** `api/system_ssh.rs:1428-1453` (ломаются на длинных путях ФС).
- **`/proc` CPU% / RSS (`4` места):** `system.rs:163-534` (`collect_linux_snapshot`, ошибочно умножает CPU% процесса на `num_cores`), `system.rs:847` (`get_stack_memory_info`), два парсера `/proc/meminfo` в одном `system.rs` (`219` и `1255`), `rci.rs:431-533` (`get_process_rss` по обрезанному `comm`, `get_proc_stats` затирает `LAST_CPU` при `<0.2s`).
- **`Base64` (`2` самописные копии):** `api/system_ssh.rs:865` и `adguard/mod.rs:16` при уже подключённом в `Cargo.toml:37` крейте `base64 = "0.22"`.
- **`urlencoding_lite` (`2` копии):** `mihomo.rs:3250` и `adguard/mod.rs:551`.
- **`gunzip` через shell:** `updater.rs:1869` вызывает `sh -c "gunzip -c ..."` при наличии `flate2 = "1"` в `Cargo.toml:45`.
- **Логика доменов:** `api/domains.rs:79-93` (`set_domains`) продублирована в `api/domains.rs:244-258` (`force_add_domain`) — оба вызывают `apply_domain_rules_for_config` дважды под `StrictXKeen`.
- **Интерфейсы роутера:** маска `br+` / `Bridge+` / `ra+` продублирована в 4 модулях (`adguard/iptables.rs:69`, `adguard/service.rs:71`, `zapret.rs:341, 644`, `antigravity.rs:824`).

---

## 3. Кластеры дублирования на Frontend (TS/TSX/CSS)

### F-D1. `3` несовместимые реализации переключателя AdGuard / AdBlock (`[F1-002]`)
- `Header.tsx`: дергает `POST /api/adguard/protection`.
- `Dashboard.tsx:268-297`: последовательно вызывает **оба** эндпоинта (`/api/adguard/protection` + `/api/adblock/toggle`), провоцируя двойной `ConfigTx` и двойной reload Mihomo.
- `Settings.tsx:241-253` & `SystemSettingsCards.tsx:84`: вызывает **только** `POST /api/adblock/toggle`.

### F-D2. `3` копии модального окна ИИ-классификатора БС (`[F1-006]`, `[F2-009]`)
- `Dashboard.tsx:925-1214`
- `settings/WhitelistDetectorSection.tsx:98-709`
- `Whitelist.tsx` (третья копия логики и разметки выбора серверов БС).

### F-D3. `7+` несогласованных форматтеров байтов и аптайма (`[F2]`, `[F3]`)
| Функция | Файл | Единицы / особенности |
|---|---|---|
| `formatBytes` | `TrafficGraph.tsx:72` | Локальный форматтер графика |
| `formatBytesRu` | `Dashboard.tsx:117` | RU (`Б/КБ/МБ`) |
| `formatBytes` | `SwapManager.tsx`, `serversHelpers.tsx` | Утилиты карточек |
| `formatBytes` | `Devices.tsx` | EN (`B/KB`) |
| `formatBytes` | `DeviceRow.tsx` | RU (`Б/КБ`) — отличается от родительского `Devices.tsx`! |
| `formatBytes` | `Gaming.tsx:111`, `ConnectionsViewer.tsx:54` | Локальные копии |
| `formatTraffic` | `vkturn/vkturnHelpers.ts` | RU (`Б/КБ`) |
| `formatRss` / `formatUptime` vs `fmtUptime` | `TaskManager.tsx:160, 172` vs `zapretHelpers.tsx:1` | Разные форматы времени для задач и Zapret |

### F-D4. Повторные вызовы браузерных диалогов, парсеров и polling-циклов
- **Блокирующий `confirm()` / `alert()`:** в `AdGuard.tsx:271`, `ConnectionsViewer.tsx:427`, `ConfigEditor.tsx:285`, `Devices.tsx:440` (на английском), `DeviceRoutingModal.tsx:86` (`window.alert`), `Settings.tsx:99, 682, 696`, `SystemSettingsCards.tsx:314, 324, 327`, `TrafficGraph.tsx:105`, `VkTurn.tsx:378, 496`, `dirtyGuard.ts:53`.
- **Дублирующиеся парсеры `host:port` / IP:** `ConnectionsViewer.tsx`, `utils/nodeParser.ts`, `utils/csqttLink.ts`.
- **Дублирующиеся polling-таймеры:** `Settings.tsx` поднимает три независимых `setInterval` (`1.8s`, `15s`, `5s`) поверх глобального пуллера `App.tsx`; `Header.tsx:201` и `useUpdateCheck.ts:117-121` независимо опрашивают GitHub каждые `90s` и `60s`.

### F-D5. Конфликтующие дубли CSS (`[F3-001]`, `[F3-002]`, `[F3-010]`)
- `.btn-whitelist`: задан и в `frontend/src/styles.css:145-149`, и в `frontend/src/styles/cards-servers.css:587-599`.
- `.status-badge-custom`: дублируется внутри `light-theme-base.css` (строки `62-65` и `190-193`).
- `.log-error` / `.log-line.error`: расщеплены между `modals-editors.css:735-737` (мёртвый селектор) и `telemetry-settings-modals.css:369`.
- Коллизия слоёв `z-index: 1000`: одинаковый `z-index: 1000` у `.header-bar`, `.header-status-popover`, `.header-stack-mem-tooltip`, `.modal-backdrop`, `.modal-overlay` и `TaskManager.tsx:702` при `999 !important` у `.task-manager-expanded`.
