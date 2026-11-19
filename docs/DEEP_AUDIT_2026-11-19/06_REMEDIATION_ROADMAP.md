# Дорожная карта исправлений, рефакторинга и тестового шлюза (`06_REMEDIATION_ROADMAP.md`)

План выстроен с учётом **`docs/REWORK_FIRST.md`** (не вносить новых фич поверх сломанного фундамента), изоляции файлов (`1` домен за раз) и строгих анти-регрессионных тестов на каждый баг и хардкод.

---

## Волна 0 — Аварийная безопасность, RCE, бэкапы и целостность данных (`P0 Security & Integrity`)

1. **Закрыть RCE в `updater.rs::mihomo_install` (`[B1-001]`, `SEC-13`):**
   - Парсить `custom_url` через строгий URL-валидатор, запрещая `..`, `?`, `#`, `@` и перенаправления с исходного домена.
   - Убрать доверие к переданному клиентом `req.sha256` для сторонних URL; требовать хеш исключительно из проверенного релизного манифеста; убрать пропуск SHA-256 при `None` в `download_file_verified_with_progress` и `setup.sh:108-124` (`[B1-011]`).
2. **Починить автооткат обновления панели (`[B1-002]`, Баг 127):**
   - Заменить `std::fs::copy(bak, bin)` (падающий с `ETXTBSY`) на копирование во временный staging-файл в той же ФС + `sync_all()` + атомарный `rename(staging, bin)`.
   - Исправить `update_guard_decision`: очищать маркер по таймеру uptime **текущего процесса панели** после успешного `TcpListener::bind`, а не от времени до рестарта (`started_at`), чтобы 5-минутный cron не стирал защиту до 3-й попытки.
3. **Защитить секреты и панель от lockout (`[B1-004]`, `[B1-005]`, `[B1-006]`, `[B6-010]`, `[F1-001]`):**
   - Убрать авто-включение `auth.enabled = true` при пустом `password_hash` в `config.rs:1650` (безопасный режим восстановления/setup вместо вечного lockout).
   - Сохранять `auth` и дергать `sync_full_tunnel_modes` в `POST /api/settings/reset` (`system_ssh.rs:1468`).
   - В `export_backup` (`backups.rs:364`) маскировать/исключать `auth`, `rci.password`, `telegram_bot_token`, `vkturn.vk_token`, `mihomo.secret` (или требовать пароль шифрования), а в `restore_backup` не перетирать `auth` без явного флага.
   - Удалить `localStorage` кэширование токена Telegram в `Settings.tsx:148-155, 278-285`.
   - В `auth.rs` убрать доверие ко всем `*.keenetic.pro`/`.link`/`.name` и single-label хостам, включить Host/CSRF проверку для `/api/auth/login` (Login-CSRF), сверять реальный CSRF-токен вместо `!= ""` и учитывать порт в Origin/Host.
4. **Закрыть YAML Injection и traversal (`[B2-006]`, `[B4-003]`):**
   - Ввести `is_valid_provider_id` в `update_provider_hwid`; пропускать `new_hwid`, `file_path` и `hc_url` через строгий `yaml_quote` (или перенести на `serde_yaml`).

---

## Волна 1 — Критические сетевые и роутерные отказы (`P0 Hardware & Networking`)

1. **Универсальная совместимость с `iptables v1.4.21` (`[B6-003]`, `T08b`):**
   - Создать единый конструктор/детектор поддержки `-w` при старте (проверить `iptables -w -L -n` / отвергнуть `-w 2` без числового аргумента) и перевести **все** скрипты и команды (`040-adguard.sh`, `gaming.rs`, `system_ssh.rs`, `zapret/*`, `antigravity.rs`) на единый helper.
   - Устранить `iptables -w 2 -w 2`, убрать слепое `2>/dev/null || true` при добавлении правил (логировать ошибки и возвращать реальный статус из `/api/adguard/sync-firewall`).
   - Исправить генерацию `/opt/etc/init.d/S51zapret` (синтаксическая ошибка `line 53` в `T09.3`) и убрать хардкод `*--qnum=200*` / `ip6`.
2. **AdGuard Home: порт, фаервол и `AdGuardHome.yaml` (`[B5-001]`–`[B5-005]`, `[B6-001]`, `[B6-002]`):**
   - Проверить конфликт UDP `5353` с `avahi-daemon` ДО первого запуска AGH и автоматически выбирать `5354`, синхронизируя `cfg.adguard.dns_port` через `ConfigTx`.
   - При старте панели проверять `cfg.adguard.integration_choice_locked` (`main.rs:609`).
   - В `adblock_toggle_core` и `adguard_set_protection` сначала проверять/применять AGH и `iptables`, и только потом фиксировать `ConfigTx` с полным откатом при сбое.
   - Перевести `adguard/yaml.rs` на структурированный YAML, убрав глобальное удаление `- 127.0.0.1` (`[B5-005]`) и неатомарный `std::fs::write` (`645`).
3. **Убрать фейковый `blockcheck` и привести `Zapret` к правде (`[B6-004]`, `[B6-005]`, `[F3-004]`):**
   - Вырезать эмуляцию баллов `23..37` при `0/4` доступных сайтах; требовать реальный перезапуск `nfqws2` (либо честно отключать кнопку при недоступности, пока не реализован реальный прогон стратегий).
   - В `zapret/diagnostics.rs:627` проверять наличие ELF `nfqws2` и выставлять `zapret.enabled` только после успешного скачивания/запуска.
4. **Разрубить узел БС / Подписки / `free_sub` / `vkturn` (`[B2-001]`–`[B2-006]`, `[B3-001]`–`[B3-007]`, `[B4-002]`, `[B5-012]`, `T02–T04`):**
   - Отделить виртуальные пулы (`is_pool_card: true`) от реальных нод во всех потребителях (`failover`, `detector`, `speedtest`, `watchdog`); резолвить активный лист из селектора `🛡️ Whitelist` / `PROXY` в один общий метод `mihomo::resolve_active_leaf`.
   - Пересобрать `add_provider` + `commit_vkturn` + `free_sub` вокруг единого пайплайна `apply_routing` (всегда добавлять активный провайдер в нужные `use:` группы — `PROXY`, `🛡️ Whitelist`, `🛡️ Canary Probe`, `🎮 Gaming`, не смешивая БС и VPS `INV-1`).
   - Исправить `mihomo.rs` working-tree патч: вернуть изоляцию `🛡️ Whitelist` (не трогать её при ручном свитче обычного VPS!) и откатывать `sync_mode_whitelist_state` при сбое `switch_server` в `free_sub`.
   - Задать глобальный `.timeout(Duration::from_secs(15..20))` и раздельные клиенты HTTP (без общего `cookie_store(true)`), чтобы `add_provider`, `scan-cdn` и `antigravity/check` не зависали на 70 секунд, плюс добавить проксированный fallback через живой `vkturn`/активный туннель для БС WAN (и починить `.txt` сиды `embedded_*_seed.txt`).
   - Исправить пробинг без прямого `1.1.1.1`/`8.8.8.8` DNS (IP-based L7 check URL вроде `http://1.1.1.1/generate_204` / DNS fallback) и убрать подмену `-1 -> 0` в `get_servers`.
   - Починить `vkturn` health-проверку (`POST /api/vkturn/action test`), исключить коллизии правил `40`/`45`/`222` (динамический выбор приоритета/таблицы) и прояснить причину TLS EOF / `TUNCONF` лимита потоков в `csqtt-client` (`[LIVE-T04-01]`).
5. **Восстановить правила фронтенда `Two-Channel`, UI/CSS и автозапуск после ребута (`[F1-002]`, `[F1-003]`, `[F2-001]`, `[F2-002]`, `[F3-005]`, `AUDIT-118`–`AUDIT-123`):**
   - Вынести единый экшн переключения AdGuard (`Header`, `Dashboard`, `SystemSettingsCards`).
   - В `ActiveServerBanner.tsx:231` убрать сложение `proxy_bytes + whitelist_bytes` и подстановку `liveTraffic?.down`; в `TrafficGraph.tsx:179` поменять порядок `h.whitelist?.active ?? data.whitelist_active`; в `Help.tsx:597-603` вернуть правильные цвета (🟢 Direct / 🔵 VPS / 🟣 БС) и актуальные имена вкладок.
   - Починить `ServerList.tsx` в компактном и карточном режимах (`FREE-VPN`/`FREE-WL`: выровнять нижнюю панель при переносе пилюли `🟣 Белый список`, убрать троеточие обрезки названия, подключить `handleFreeSubConnect`, скрыть `btn-speedtest`, убрать дубли заголовка из `КОСЯКИ.md`).
   - Исправить живые дефекты `T11`: закрытие модалки `Редактор конфигов` по `Escape` / «Отмена» / `✕` / фону (`AUDIT-118`), ширину `input[type=number]` в блоке Failover настроек (`AUDIT-119`), инверсию цветов каналов и низкий контраст Светлой темы (`AUDIT-120`), горизонтальный скролл и смешанную EN/RU локализацию в `Устройствах` + переключение «Карта политик» / клики вкладок из `#connections` (`AUDIT-121`), а также компактную мобильную шапку `390x844` и противоречия бейджей (`AUDIT-123`).
   - Обеспечить штатный автозапуск `mihomo` после перезагрузки роутера (установка/проверка `S05xkeen` или запуск из `S99xkeen-route`, `AUDIT-122`).

---

## Волна 2 — Конкурентность, блокировки и единая транзакционность (`P1 Concurrency & Locks`)

1. Закрыть все прямые записи `config.json`/`config.yaml` в `ConfigTx` (`policy_isolation.rs`, `failover/probe.rs`, `updater.rs`, `zapret/diagnostics.rs`, `subscriptions.rs::refresh_mobile_sub`), и ограничить `clash_proxy` (запрет не-read-only обходов конфигурации).
2. Гарантировать порядок локов: **`config_lock -> routing_lock -> subsystem_lock (zapret_lock / speedtest_lock)`**; устранить обратный порядок в `watchdog.rs:1078` и гонку `stop`/`stop-fw` до `zapret_lock`; никогда не держать `config.read().await` через `.await` сетевого/дискового I/O (`main.rs`, `clash_proxy.rs`).
3. Вынести все `std::process::Command` (`df`, `swap`, `ndmc`), `std::fs::sync_all` и скан `/proc` в `tokio::process` / `spawn_blocking`.
4. Исправить `apply_with_confirmation` (`transaction.rs:1053`): снимать `applied_*` внутри транзакции до `drop(tx)`, при `rollback_seconds == 0` отменять активный `PendingCommit`, и чинить `validate_config_dry_run` (`port == "80"` вместо `starts_with`).
5. Вернуть **Instant Domain Save (< 1s)**: убрать скрытые блокирующие шаги из `POST /api/domains` (`T10.1b` занял `7.941s`) — при сохранении применять только `expand_bundles` + быстрый reload без пересборки лишних блоков.
6. Восстановить приоритет встроенного `Assets::get` над `/opt/share/xkeen-route/dist` в `frontend.rs` (`[B1-003]`).

---

## Волна 3 — Дедупликация и декомпозиция спагетти (`DRY & Clean Architecture`)

| Консолидируемый кластер | Целевой единый модуль | Что удаляется / сливается |
|---|---|---|
| **Перепутаница `routing/*`** (`[B4-001]`) | `routing/{mod,yaml_doc,providers_yaml,rules_domains,rules_whitelist,rules_gaming,netfilter_iptables,netfilter_ipset}.rs` | Переместить `routing/mod.rs` тесты в `#[cfg(test)]` подмодулей или `tests/`; дать файлам честные имена; заменить строковый парсер на `serde_yaml` / единый marker-engine |
| **Запись файлов (`6` копий D5)** | `fs_atomic::{write_atomic_async, write_atomic_sync}` | Слить `transaction.rs:52`, `auth.rs:917`, `override_sync.rs`, `traffic.rs`, `failover/probe.rs`, `watchdog.rs`, `adguard/yaml.rs` |
| **Свитч/пинг/БС-детектор (`D1–D3`)** | `mihomo/switcher.rs`, `mihomo/probe.rs`, `whitelist_tags.rs` | Слить `7` входов переключения, `6` L7-пингеров и `3+` детектора БС |
| **Системные парсеры (`D7–D8`)** | `system/procfs.rs` + крейты `base64`, `flate2` | Один парсер `df`, один парсер `/proc` CPU%/RAM/ARP, удалить самописные `base64_encode`, `gunzip` shell и `urlencoding_lite` |
| **Циклы `watchdog.rs` (`9` петель)** | `watchdog/{supervisor,mihomo,dhcp,adguard,zapret,gaming,flow,schedules}.rs` | Единый `DaemonLoop` с `shutdown::subscribe()` и лимитами ретраев (без 5-кратного спама при выключенном Zapret в `T01`) |
| **React монолиты & форматы (`F-D1..F-D5`)** | `utils/format.ts`, `utils/network.ts`, `components/common/WhitelistClassifierModal.tsx`, `components/common/ConfirmDialog.tsx` | Убрать `7+` копий `formatBytes`, `3` копии ИИ-классификатора, `9+` `window.confirm/alert`, разрезать монолиты >1000 LOC, очистить дубли CSS и выстроить единую шкалу `z-index` |

---

## Волна 4 — Починка CI и реальный анти-регрессионный тестовый шлюз (`Tests That Prove Truth`)

1. **Убрать `|| true` в `.github/workflows/ci.yml`** (`cargo fmt --check`, `cargo clippy`, `cargo test`) и синхронизировать `build.yml` + релизы (один финальный job создает `latest` после сборки всех архитектур, включая MIPS).
2. **Удалить/переписать «липовые» тесты:**
   - Заменить self-matching `backups.rs:608` (`include_str!`), фиктивные локальные лямбды (`updater.rs:2347`, `traffic.rs:1078`, `failover/mod.rs:5231`), заглушку Linux-процессов (`system.rs:1075`) и глобальную мутацию `SHUTDOWN` (`failover/mod.rs:1711`).
   - В `antigravity.rs` подключить `diff_ip_host_rules` в реальный код или удалить мёртвую функцию.
3. **Обязательный набор новых анти-регрессионных тестов на ключевые находки:**
   - **Backend (Axum + mock Mihomo TCP):** `mihomo_install` отбивает `/../` и подмену `req.sha256`; `update_guard` переживает `ETXTBSY`-сценарий (atomic stage+rename) и 5-мин cron; `reset_settings` сохраняет `auth`; `export_backup` не отдаёт raw токены/хеши; `add_provider` добавляет подписку в `PROXY`/`🛡️ Whitelist`/`Canary Probe`; `free_wl`/`free_vpn` проходят полный путь через `failover` + `whitelist-detector` + `speedtest`; `update_provider_hwid` экранирует YAML; `apply_routing` в global БС сохраняет pinned device; `tests` на инвариант `NO iptables -w <num>` и отсутствие `-w 2 -w 2`.
   - **Linux VM (`iptables v1.4.21` / `busybox ash`):** обязательный интеграционный прогон сгенерированных `040-adguard.sh` и `S51zapret` на совместимость с синтаксисом `ash` (ловит `line 53 syntax error`) и реальным вставкой/удалением цепочек в `nat`/`mangle`.
   - **Frontend (Vitest):** `Settings` не хранит и не воскрешает Telegram-токен в `localStorage`; единый тоггл AdGuard во всех 3 местах; `ActiveServerBanner` не суммирует Direct+БС+VPS; `TrafficGraph` сохраняет исторический `h.whitelist.active`; `OutboundGeneratorModal` импортирует `csqtt://` + `vless://` без потери; `ServerList` в компактном режиме правильно подключает `FREE-WL`/`FREE-VPN`.
   - **Стенд `192.168.1.1`:** baseline-откат `T12` уже выполнен и верифицирован (`T12_rollback_verify.log`, 100% parity); после внесения исправлений заново прогнать live `T01–T12` + браузерный аудит (Light/Dark + `390x844` + конфиг-редактор и числовые поля Failover).
