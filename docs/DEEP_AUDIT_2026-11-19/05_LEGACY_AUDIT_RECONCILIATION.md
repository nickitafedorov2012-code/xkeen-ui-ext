# Сверка со старыми аудитами и реестрами (`REWORK_FIRST`, `BUG_AUDIT`, `FIX_LEDGER`, `MASTER_AUDIT_REGISTRY`, `КОСЯКИ.md`)

Данный отчёт сопоставляет все исторические аудиторские документы проекта с **фактическим кодом на диске (`HEAD ac0368e` + working tree)** и **результатами стендовых проверок на `192.168.1.1`**.

---

## 1. Статистика старых журналов vs реальность (`scratch/legacy_141_summary.json`)

| Источник | Заявленный объём | Фактический разбор |
|---|---:|---|
| **`docs/BUG_AUDIT.md`** | `141` нумерованный баг (`1..141`) | Проверена полнота парсинга: `141 / 141`, пропусков нет (`missing_in_ba = []`) |
| **`docs/FIX_LEDGER.md`** | `95` строк в сводке | **`87 FIXED`** + **`8 BLOCKED`** (IDs: `11, 16, 31, 38, 42, 45, 69, 115`), **`46` пунктов не начато** (`32,6%` от 141 не затронуто) |
| **`docs/REWORK_FIRST.md`** (ревизия `2026-11-05`) | Шаги `0–4` | Выявлен рассинхрон с `FIX_LEDGER`: пункты `18, 40, 43` (и `FIXED` из шага 1: `1, 13, 36, 37, 70, 91, 114`) значатся в `REWORK_FIRST`, но по коду **часть из них всё ещё не доделана или содержит регрессии** |
| **`docs/MASTER_AUDIT_REGISTRY.md`** | `61` дефект (`14 SEC, 26 BUG, 12 FE, 9 HC`) | Несколько критических пунктов (`SEC-03` в `hwid`, `SEC-08`, `SEC-13`, `BUG-05`, `BUG-11`, `BUG-20`, `BUG-21`) остались открытыми или закрыты лишь частично |
| **`КОСЯКИ.md`** | `11` живых симптомов | Подтверждены в коде и/или вживую на `192.168.1.1`; локальные правки working tree закрыли их лишь фрагментарно |

**Список `46` неначатых пунктов в `docs/FIX_LEDGER.md` (`not_in_ledger_1_141`):**  
`19, 20, 21, 22, 23, 34, 35, 46, 47, 48, 49, 50, 52, 53, 54, 55, 60, 63, 65, 66, 74, 75, 78, 79, 87, 99, 100, 101, 104, 105, 106, 107, 112, 113, 117, 121, 123, 124, 126, 127, 129, 132, 137, 138, 139, 140`.  
*(Важно: часть из них — например, `78` (`percent_decode`), `127` (`update_guard`), `129` (`speedtest_lock` в боте), `132` (`config.rs:1469`) — имеют частичные правки в коде без записей в `FIX_LEDGER`, причём `127` остаётся сломан на Linux из-за `ETXTBSY` и cron.)*

---

## 2. Проверка `docs/REWORK_FIRST.md` (Шаги 0, 1, 4 и предупреждения)

| Пункт `REWORK_FIRST` | Статус в `FIX_LEDGER` | Реальный статус при перепроверке | Доказательство |
|---|---|---|---|
| **Шаг 0 / Баг 37** (`zapret.rs` failsafe & LAN check) | `FIXED` | **FIXED_VERIFIED** (в рамках локальных тестов, с поправкой на `BUG-T`) | Обновлён `2026-11-06`: функции `is_failsafe_http_status_valid` и `has_lan_bridge_interface` присутствуют; в `B5-015` осталось регистрозависимое сканирование `br*`/`Bridge*` |
| **Шаг 1 / Баг 1** (AdGuard dead port / fallback) | `FIXED` | **PARTIAL / STILL BROKEN LIVE (`[B3-011]`, `[B5-003]`, `[B6-002]`)** | При `mihomo_down && !listener_ok` `watchdog.rs:1307` всё ещё направляет DNS на мёртвый порт Mihomo; ошибки `tx.commit_and_reload()` проглочены (`let _ =` на `1253`); конфликт `avahi` на `5353` (`T08`) |
| **Шаг 1 / Баг 13** (`override_sync` `&[]` затирает CDN) | `FIXED` | **FIXED_MOSTLY (`[B4]`/`[B6-011]` оговорки)** | `CACHED_AUTO_CDNS` сохраняет сканированные CDN, однако `main.rs:820` держит read-лок `config` на время I/O, а `api/domains.rs:273` шлёт `&[]` вместо вычисленных доменов |
| **Шаг 1 / Баг 36** (`is_zapret_pid` + watchdog гонка) | `FIXED` | **PARTIAL (`[B3-010]`)** | `is_zapret_pid` проверяет cmdline (`S51zapret:385`), но `watchdog.rs:926-959` всё ещё дёргает `stop`/`stop-fw` **до** `zapret_lock`, а `1078` создаёт риск ABBA с `ConfigTx` |
| **Шаг 1 / Баг 70** (`iptables` вызовы без `-w`) | `FIXED` | **STILL BROKEN LIVE (`[B6-003]`, [T08b])** | В старом роутере `iptables v1.4.21` отвергает `-w 2` (`Bad argument '2'`), а в `system_ssh.rs:538,555` и `zapret/firewall.rs:366,807` генерируется двойное **`iptables -w 2 -w 2`**, приводя к `0` правил `XKEEN_ADGUARD` в `T08b`! |
| **Шаг 1 / Баг 91** (DNS-rebinding GET/POST) | `FIXED` | **PARTIAL (`[B1-004]`)** | `evil.com` закрыт, но `*.keenetic.pro`/`.link`/`.name`, single-label домены и `/api/auth/login`/`status` (`is_exempt`) всё ещё открыты; `X-Forwarded-Proto` доверяется слепо |
| **Шаг 1 / Баг 114** (AdGuard YAML перезапись & fallback DNS) | `FIXED` | **STILL BROKEN (`[B5-004]`, `[B5-005]`)** | `ensure_default_adguard_config` пишется неатомарно (`std::fs::write`); `sanitize_and_heal_adguard_yaml` (`475-483`) удаляет `- 127.0.0.1` глобально из посторонних секций YAML |
| **Шаг 4 / Weekly CDN lock** (`main.rs`) | Не в ledger | **FIXED_WITH_FAKE_TEST (`[B1-008]`)** | В еженедельном CDN-скане (`main.rs:855-860`) read-лок сбрасывается до сети, но тест `regression_bug_a07` (`1028`) фиктивен, а в 4 соседних стартовых задачах (`main.rs:532, 547, 737, 820`) лок всё ещё держится на I/O |
| **Шаг 4 / `backups.rs` контракты R01–R06** | Не в ledger | **FAKE TEST (`[B6-T01]`)** | `regression_r01_r02_r03_r06_backup_contracts` (`backups.rs:608`) сканирует собственный текст модуля тестов |

---

## 3. Сверка с `docs/BUG_AUDIT.md` (дополнительные ключевые ID сверх Шага 1)

| `BUG_AUDIT` ID | Суть | Статус в коде / на роутере |
|---|---|---|
| **4** | Восстановление gaming при старте с БС | **FIXED_VERIFIED** (`main.rs:663-764` ждёт Mihomo и использует `startup_gaming_restore_allowed`) |
| **11** | Lockout при битом `config.json` / WATCHDOG | **BLOCKED в ledger / STILL BROKEN (`[B1-005]`)**: `config.rs:1650` ставит `auth.enabled=true` при пустом хеше пароля |
| **18** | Antigravity bind LAN vs защита | **PARTIAL (`[B5-016]`)**: слушает `0.0.0.0:53129` (`T10.2b`), но `! -i br+ DROP` не переносится на `Bridge+` и нет NDM-хука |
| **19–22** | Режимы Antigravity, DNS `0xC0`, подсети Google | **UNSTARTED / STILL BROKEN (`[B5-016]`–`[B5-018]`)**: `active_ip` обходит route, DNS fallback в blocked `1.1.1.1:53` (`T10.2c` >20s) |
| **34, 35** | Zapret `qnum=200` хардкод, двойная сериализация `conf` | **UNSTARTED / PARTIAL (`[B6-011]`)**: `NDM_NETFILTER_SCRIPT:43` всё ещё хардкодит `*--qnum=200*` |
| **56** | `rci::set_clean_dns_servers` только Яндекс DNS | **FIXED_VERIFIED** (`rci.rs:862, 909-941`) |
| **68 / 69** | Обязательность SHA-256 и подпись обновлений | **68 PARTIAL** (обходится `req.sha256` в `mihomo_install` + `setup.sh:109` пропускает без проверки `[B1-001]`, `[B1-011]`); **69 BLOCKED** |
| **76–79** | `subscription_parser` SS/VMess/quoting/transports | **76 FIXED**, **77/79 PARTIAL (`[B2-006]`, `[B2-007]`)**: `parse_xray_json_profile` и `parse_trojan_uri_checked` теряют поля, `update_provider_hwid_in_yaml` не экранирован |
| **84–86** | Speedtest изоляция (`Canary Probe`) | **PARTIAL (`[B2-005]`)**: падает на `device_providers` и `paid_wl` (`400 proxy not exist` в `T02`), хардкод `127.0.0.1` |
| **90, 92, 93** | Speedtest POST, PBKDF2 `spawn_blocking`, отзыв токенов | **FIXED_VERIFIED** (`auth.rs:74-238`, `router.rs:75`) |
| **94** | `config::save` `fsync` | **PARTIAL (`[B1-009]`)**: `config::save` синкает (синхронно!), но `transaction.rs:63` и `auth.rs:918,940` пишут без `sync_all()` |
| **104–107** | Гонки детектора БС и перезапись ручного БС | **UNSTARTED / STILL BROKEN (`[B3-001]`–`[B3-007]`)**: подтверждено живым застреванием детектора в `T00` и противоречием `T05`/`T06` |
| **115, 116** | `AdGuardHome.yaml` и `integration_choice_locked` | **PARTIAL / REGRESSION**: `main.rs:609` всё ещё принудительно ставит `adguard.enabled = true` на старте без проверки `integration_choice_locked` (`[B1-008]`) |
| **122, 123, 124** | Switch БС перетирает группы, auto-heal, 3 детектора AI | **122 REGRESSED IN WORKING TREE (`[B2-002]`)**; **123/124 UNSTARTED (`[B2-009]`, `[B4-004]`)** |
| **125** | Инъекция путей в `system.*` | **FIXED_VERIFIED** (`config.rs:1477-1520`) |
| **127** | Автооткат обновления панели `.bak` | **UNSTARTED / BROKEN (`[B1-002]`)**: `std::fs::copy` падает с `ETXTBSY`, cron `*/5` обнуляет маркер до 3 попыток |
| **130, 131** | Восстановление `.bak`, `commit-confirmed` | **PARTIAL (`[B1-005]`, `[B1-009]`)**: окно гонки после `drop(tx)` + `rollback_seconds == 0` не снимает прошлый таймер |
| **137–140** | Domain UI race, Update polling дубли | **UNSTARTED (`[F1-005]`, `[F3-007]`)**: `Header.tsx` (90с) и `useUpdateCheck.ts` (60с) оба дёргают GitHub; `UpdateModal` дублирует кнопки |

---

## 4. Сверка с `docs/MASTER_AUDIT_REGISTRY.md` (`SEC-*`, `BUG-*`, `FE-*`, `HC-*`)

- **Подтверждённые живые пробелы `SEC` / `BUG` из 61 пункта реестра:**
  - **`SEC-03` (Path Traversal / YAML injection):** закрыт в `add/delete_provider`, но **остался в `update_provider_hwid`** (`[B2-006]`) без `is_valid_provider_id` и экранирования.
  - **`SEC-08` (CSRF / DNS-rebinding `*.keenetic.pro`):** не закрыт полностью (`[B1-004]`).
  - **`SEC-13` (RCE через `custom_url` в `mihomo_install`):** **STILL BROKEN (`[B1-001]`)** — обход `/../` + переопределение `req.sha256` + фейковый тест `updater.rs:2347`.
  - **`BUG-03` (`ConfigTx` в провайдерах):** остался незакрыт в `refresh_mobile_sub`, `clash_proxy`, `policy_isolation`, `failover/probe`, `updater`, `get_zapret_analytics`.
  - **`BUG-04` (`import_node` `proxies: []`):** ломается на `proxies: [] # comment` (`[B2-006]`).
  - **`BUG-05`, `BUG-11`, `BUG-20`, `BUG-21`:** остаются частично сломанными (`[B1-005]`, `[B1-010]`, `[B1-013]`, `[B1-014]`).
  - **`BUG-09` (`run_mini_blockcheck`):** **STILL BROKEN LIVE (`[B6-004]`, [T09.4])**.
  - **Регресс `frontend.rs` (`xkeen-troubleshooting`):** диск `/opt/share/xkeen-route/dist` снова приоритетнее встроенных `Assets::get` (`[B1-003]`).

---

## 5. Проверка всех 11 пунктов `КОСЯКИ.md`

| # | Пункт `КОСЯКИ.md` | Результат проверки на `192.168.1.1` и в коде |
|---:|---|---|
| **1** | `csqtt-client` не вшит и падает с GitHub (`direct`) | **Подтверждён live (`T04.1`)** и в коде (`[B5-008]`): только `arm64`, только GitHub без зеркал |
| **2** | В устройствах везде по умолчанию запрет | **Подтверждён live (`T11`)** и в коде: на чистом baseline стенде (`zapret.installed=false`) у устройства `DESKTOP-CQCUM2G` (`192.168.1.115`) активна кнопка `⚡ Zapret ON`, а `routing/policy_isolation.rs` изолирует `default`-политику по `xkeen_deny_mac` и `StrictXKeen` |
| **3** | Ошибка «не входит ни в одну переключаемую группу» (`🛡️ Whitelist` / `geodema`) | **Подтверждён live (`T00`, `T02_loopback_paid.log:394`)** и в коде (`[B2-002]`, `[B2-006]`): на `beta.11.0` не исправлено, локальный патч в working tree ломает изоляцию БС |
| **4** | Пинг `0` при недоступном прямом DNS (`1.1.1.1`/`8.8.8.8`) | **Подтверждён live (`T02`, `T03`)** и в коде (`[B2-004]`): доменные `health_check_url` не резолвятся при прямом таймауте DNS |
| **5** | VK-туннель: зависание на `tcp` и обрыв TLS при MTU 900 | **Частично уточнён live (`T04`, `T10b`)**: TCP подключил `20/20`, но сыпал `TUNCONF: превышен лимит потоков`, `GETCONF timeout` и ложный провал `action: "test"`; **обрыв `curl (35) TLS unexpected eof` подтверждён live**; локальный `vkturn` до сих пор не добавляется в `🛡️ Whitelist`/`🎮 Gaming` (`[B5-012]`) |
| **6** | Спидтест по `free_wl`/`free_vpn` -> `400 Bad Request` | **Обновлён live (`T03.4d`)**: пустая карта возвращает сообщение «сначала нажмите Подключить», но сам `speedtest` платной ноды `paid_wl` по-прежнему падает с **`400 Bad Request: proxy not exist`** (`[B2-005]`, `T02_loopback_paid.log:442`) |
| **7** | Автоинсталлер не поднимает Entware с пустым паролем (`WBR3000UAX`) | Внешний репозиторий `xkeen-auto-installer`; подтверждено правило обхода `/rci/opkg` напрямую через SSH |
| **8** | Перепутаны `↓` и `↑` в `ActiveServerBanner.tsx` / трафике | **Подтверждён разрыв в коде (`[B4-007]`, `[B6-007]`)**: `devices.rs:595` инвертирует `upload<->download`, `gaming.rs:1038` не инвертирует, а `ActiveServerBanner.tsx:231` суммирует каналы (`[F2-001]`) |
| **9** | Кнопка «Запустить» (`/api/xkeen/service` `start`) не запускает остановленный Mihomo | **Подтверждён live (`T01`, `56–103`)**: на `beta.11.0` не исправлено; в working tree (`system_ssh.rs:640`) для `action=="start"` заменён на `commit_without_reload` |
| **10** | Кривая вёрстка карточек `FREE-VPN`/`FREE-WL` в `ServerList.tsx` | **Подтверждён статически (`[F2-002]`) и live (`T11`, `final_step_93.png`)**: обрезка `Бесплатный обход БС (А...` многоточием, перенос `🟣 Белый список` на 2-ю строку со смещением нижней панели действий относительно `FREE-VPN`, красные столбики сигнала рядом с `🟢 Flow OK` |
| **11** | Кнопка `FREE-WL`/`FREE-VPN` «ничего не делает», поиск >6 мин с `0` | **Подтверждён live (`T03`)** и в коде (`[B2-001]`, `[B2-003]`): `l7_passed: 0`, >`55.7s` застревания, проглоченная ошибка `switch_pool_node`, гонка `running: false` |
