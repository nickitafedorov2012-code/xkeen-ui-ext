# Глубокий аудит XKeen Route (`2026-11-19`) — Итоговая сводка

**Объект проверки:** [`xkeen-ui-ext-main`](file:///c:/Users/internet/Desktop/xkeen%20route/xkeen-ui-ext-main) (`HEAD ac0368e`, `v1.17.0-beta.11`)  
**Режим работы:** строго **read-only** по исходному коду (отслеживаемые файлы не изменялись) + живые стендовые испытания на тестовом роутере **`192.168.1.1`** (`WBR3000UAX / KN-4110`, KeeneticOS `5.0.11`, `v1.17.0-beta.11.0`, Mihomo `v1.19.33`) с полным откатом к исходному состоянию (**`T12` PASS**). Основной роутер **`192.168.2.1`** не затрагивался, пароли роутера не менялись.

---

## 1. Масштаб проверенного проекта и базовые метрики

| Слой | Объём исходников | Базовые прогоны тестов | Реальное покрытие / оговорки |
|---|---:|---|---|
| **Backend (Rust)** | **`67`** `.rs` файлов, **`80,395` LOC** (`32` файла `≥ 1,000` LOC) | `cargo test`: **`799 passed; 0 failed`** (`46.30s`) | Часть тестов проверяет локальные лямбды/заглушки (`backups.rs`, `updater.rs`, `failover/mod.rs`, `system.rs`), а `.github/workflows/ci.yml` глушит провалы через `\|\| true` |
| **Frontend (TS/TSX)** | **`129`** файлов, **`49,639` LOC** (`83` prod `37,138` LOC + `46` тест `12,501` LOC) | `vitest run`: **`46 passed (46), 386 passed (386)`** (`22.94s`); `tsc --noEmit`: **`0 errors`** | Моки (`AdGuard.test.tsx`, `Gaming.test.tsx`, `App.test.tsx`) скрывают `iptables v1.4.21`, коллизию `avahi:5353` и live CSS/DOM-баги |
| **Стили (CSS)** | **`12`** файлов, **`8,681` LOC** | Собрано в живой панели на `192.168.1.1` | Коллизии `z-index: 1000`, дубли `.btn-whitelist` / `.status-badge-custom`, инверсия цветов трафика и схлопнутые `input[type=number]` в Светлой/Тёмной темах |
| **Инфраструктура & CI** | `setup.sh`, `.github/workflows/{build,ci,csqtt-core}.yml` | `100%` инспекция | См. `|| true` в `ci.yml`, пропуск SHA-256 при `None` в `setup.sh` |
| **Итого** | **`138,715` LOC** | **100% статический + live-стенд `T00–T12`** | Дедуплицировано по коду и железу |

---

## 2. Итоговая статистика дефектов и ответ на «500+ багов»

- **Всего канонических кластеров дефектов:** **`123`** (`AUDIT-001` — `AUDIT-123` в [01_BUG_REGISTRY.md](file:///c:/Users/internet/Desktop/xkeen%20route/xkeen-ui-ext-main/docs/DEEP_AUDIT_2026-11-19/01_BUG_REGISTRY.md)):
  - **Статический аудит:** **`106`** (`79` Backend `B1–B6` + `27` Frontend/CSS `F1–F3`)
  - **Впервые доказаны вживую на `192.168.1.1`:** **`17`** (`11` API/SSH `T00–T10b` + `1` отсутствие автозапуска `mihomo` после ребута + `5` живой браузерный UI-аудит `T11`)
- **Распределение по критичности (`20 + 68 + 31 + 4 = 123`):**
  - **P0 (Критичные: RCE, обрыв сети/фаервола, утечка секретов, потеря состояния):** **`20`**
  - **P1 (Высокие: блокировки, гонки, поломки БС/подписок/UI-форм/модалок):** **`68`**
  - **P2 (Средние: дубли, противоречия статусов, мобильная шапка, несогласованность API):** **`31`**
  - **P3 (Минорные: мёртвый код, старые дефолты `geodema2`):** **`4`**
- **Кластеры дублирования логики:** **`13`** групп (`D1–D8` на Backend + `F-D1–F-D5` на Frontend/CSS в [03_DUPLICATION_CATALOG.md](file:///c:/Users/internet/Desktop/xkeen%20route/xkeen-ui-ext-main/docs/DEEP_AUDIT_2026-11-19/03_DUPLICATION_CATALOG.md)).
- **Почему ощущение наставника про «500+ багов» не противоречит цифре `123`:**
  1. Десятки кластеров в `01_BUG_REGISTRY.md` являются **составными** (объединяют `3–8` атомарных дефектов и десятки конкретных битых артефактов — например, `AUDIT-005` закрывает `6` поломок конвейера `free_sub` + `~30` битых VLESS URI в сидах; `AUDIT-084` и `AUDIT-098` объединяют по `6–7` дефектов).
  2. По историческим журналам (см. [05_LEGACY_AUDIT_RECONCILIATION.md](file:///c:/Users/internet/Desktop/xkeen%20route/xkeen-ui-ext-main/docs/DEEP_AUDIT_2026-11-19/05_LEGACY_AUDIT_RECONCILIATION.md)) в `docs/BUG_AUDIT.md` (`141` пункт) числится **`46` неначатых (`32,6%`) + `8` `BLOCKED`** (`11, 16, 31, 38, 42, 45, 69, 115`) в `docs/FIX_LEDGER.md` (`95` строк: `87 FIXED`, `8 BLOCKED`), плюс ряд `FIXED` пунктов реально сломан в коде и на железе (`Баг 13, 70, 91, 114, 122, 127` и др.).
  3. **`7` незакоммиченных файлов в working tree** (`router.rs`, `system_ssh.rs`, `free_sub/mod.rs`, `mihomo.rs`, `validator.rs`, `SystemSettingsCards.tsx`, `КОСЯКИ.md`) привносят новые регрессии поверх `v1.17.0-beta.11` (например, `reset_settings` сбрасывает `auth`, `mihomo.rs` снимает `cfg.whitelist.enabled`, ломая изоляцию `🛡️ Whitelist` при обычных VPS-переключениях).

---

## 3. Топ-10 самых критичных системных поломок

1. **RCE от `root` при установке ядра Mihomo (`AUDIT-001` / `[B1-001]`):** `updater.rs::mihomo_install` проверяет `custom_url` только префиксом `starts_with(...)` (обходится через `/../`) и принимает клиентский `req.sha256`.
2. **Нерабочий автооткат обновления панели (`AUDIT-002` / `[B1-002]`):** `std::fs::copy(bak, bin)` по работающему бинарнику падает с `ETXTBSY`, а 5-минутный cron стирает `update-guard.json` до 3-й попытки.
3. **AdGuard Home `53 -> 5353/5354` полностью молчит в `iptables` (`AUDIT-013`, `AUDIT-019` — Live `T08b.3`):** встроенный `iptables v1.4.21` отвергает `-w 2` (`Bad argument '2'`), `2>/dev/null || true` прячет ошибку, `/api/adguard/sync-firewall` рапортует `success: true` при **`0` правилах `XKEEN_ADGUARD`**; вдобавок UDP `5353` на KeeneticOS `5.0.11` занят системным `avahi-daemon`.
4. **Провал подписок и «белых списков» БС WAN (`AUDIT-003`–`AUDIT-006`, `AUDIT-081` — Live `T00`, `T02`, `T03`):**
   - `paid_wl` (`Geodema-WL`) при добавлении не прописывается в `use:` групп `PROXY` и `🛡️ Whitelist` («не входит ни в одну переключаемую группу»), `state.http` без общего `.timeout(...)` вешает `add_provider` (>70s), а `speedtest` даёт `400 proxy not exist`.
   - `free_wl`/`free_vpn` при недоступном прямом DNS (`1.1.1.1:53` timeout) дают **`l7_passed: 0`**, а пулы возвращают виртуальные ID с `is_pool = false`, ломая детектор БС, `failover` и `free_sub`.
5. **Сломанный Zapret 2 и фейковый Blockcheck (`AUDIT-014`, `AUDIT-020` — Live `T09`):** сгенерированный `/opt/etc/init.d/S51zapret` падает на запуске (`line 53: syntax error: unexpected word (expecting ")")`), а `/api/zapret/blockcheck` выдаёт фиктивные `score: 23..37` и `is_best: true` при `0/4` доступных ресурсах (`youtube_ok: false, discord_ok: false`).
6. **Отсутствие автозапуска `mihomo` после перезагрузки роутера (`AUDIT-122` — Live post-reboot):** в `/opt/etc/init.d/` отсутствует `S05xkeen` (автостартуют только `S99xkeen-route`, `S99xkeen-ui`, `S51dropbear`, `S10cron`), поэтому после ребута ядро остаётся выключенным, а `/api/xkeen/service start` на `beta.11.0` падает из-за того же отсутствующего `S05xkeen` (`AUDIT-080`).
7. **VK-туннель (`csqtt-client`, `179.253.72.27` — Live `T04`, `T10b`):** нативный `install_core` виснет на прямом GitHub; после доставки бинарника с ПК UDP пробивает выход наружу (`ipify = 179.253.72.27`), но в TCP сыпется `TUNCONF: превышен лимит потоков`, `action: "test"` даёт ложный отказ при работающем выходе, а HTTPS через `csq0` даёт **`curl (35) TLS unexpected eof`**; сам провайдер `vkturn` не добавляется в `🛡️ Whitelist` и `🎮 Gaming` (`AUDIT-010`, `AUDIT-018`).
8. **Утечка секретов и lockout панели (`AUDIT-015`, `AUDIT-016`, `AUDIT-022`, `AUDIT-023` — Live `T10b.1`):** `.xkbak` экспортируется открытым JSON со всеми ключами/паролями; Telegram-токен кешируется в открытом `localStorage`; `config::load()` может включить auth при пустом хеше, а `auth.rs` доверяет любым `*.keenetic.pro` и не проверяет CSRF на `/api/auth/login`.
9. **Критические баги живого UI (`AUDIT-118`–`AUDIT-121`, `AUDIT-123` — Live `T11`):** незакрываемая модалка «Редактор конфигов» (`.editor-backdrop`), схлопнутые числовые поля Failover в Настройках, инверсия синей/зелёной окраски каналов в Светлой теме, низкий контраст Светлой темы (`🔌`, `🟣 Белый список`), горизонтальный overflow таблицы `Устройства`, нарушение `Two-Channel` в `ActiveServerBanner` (`AUDIT-017`) и `339.6px` шапка на мобильном `390x844`.
10. **Спагетти-архитектура и рассинхрон блокировок (`02_SPAGHETTI_AND_ARCHITECTURE.md`, `03_DUPLICATION_CATALOG.md`):** перепутанные имена файлов `routing/*`, ручное построчное редактирование YAML в 10+ местах, `ConfigTx`-обходы, обратный порядок `zapret_lock <-> ConfigTx` (риск ABBA), синхронные `std::process::Command` на воркерах Tokio, `7` входов переключения серверов, `6` L7-пингеров, `6` копий атомарной записи и `3` разных пути переключения AdGuard.

---

## 4. Сводка живых испытаний на `192.168.1.1` (`T00–T12`)

- **`T00–T10b` (API / SSH / реальный БС WAN):** все сценарии выполнены в [04_LIVE_ROUTER_TEST_REPORT.md](file:///c:/Users/internet/Desktop/xkeen%20route/xkeen-ui-ext-main/docs/DEEP_AUDIT_2026-11-19/04_LIVE_ROUTER_TEST_REPORT.md); выявлены P0-сбои `iptables -w 2`, `avahi-daemon:5353`, `S51zapret: line 53`, фейковый `blockcheck`, несвязанность `paid_wl`, DNS timeout `free_sub` (`l7_passed: 0`), `7.941s` сохранения доменов и plaintext `.xkbak`.
- **`T11` (Read-only браузерный E2E `http://192.168.1.1:1001/`):** выполнен субагентом `048fba0b` (`1280x800` Dark/Light + `390x844` Mobile, верифицирован по `BROWSER_HOST_FACTS`, `final_step_93.png` и прямым замерам DOM) без мутации настроек на роутере.
- **`T12` (Откат к baseline):** выполнен скриптом `t12_rollback.py` и подтверждён логом `T12_rollback_verify.log` (`:1001/api/status` HTTP 200 за `0.11s`, 100% parity всех флагов и 5 бэкапов, `0` лишних процессов, `0` цепочек `XKEEN_*`/`GAME`, `0` временных файлов).

---

## 5. Навигация по полному комплекту отчётов (`docs/DEEP_AUDIT_2026-11-19/`)

1. **[00_EXECUTIVE_SUMMARY.md](file:///c:/Users/internet/Desktop/xkeen%20route/xkeen-ui-ext-main/docs/DEEP_AUDIT_2026-11-19/00_EXECUTIVE_SUMMARY.md)** — итоговая сводка (этот документ)
2. **[01_BUG_REGISTRY.md](file:///c:/Users/internet/Desktop/xkeen%20route/xkeen-ui-ext-main/docs/DEEP_AUDIT_2026-11-19/01_BUG_REGISTRY.md)** — полный реестр `AUDIT-001` — `AUDIT-123` (`P0=20, P1=68, P2=31, P3=4`)
3. **[02_SPAGHETTI_AND_ARCHITECTURE.md](file:///c:/Users/internet/Desktop/xkeen%20route/xkeen-ui-ext-main/docs/DEEP_AUDIT_2026-11-19/02_SPAGHETTI_AND_ARCHITECTURE.md)** — причины спагетти-кода, монолиты ≥ 1,000 LOC, путаница `routing/*`, иерархия локов и качество тестов
4. **[03_DUPLICATION_CATALOG.md](file:///c:/Users/internet/Desktop/xkeen%20route/xkeen-ui-ext-main/docs/DEEP_AUDIT_2026-11-19/03_DUPLICATION_CATALOG.md)** — `13` кластеров дублирования (`D1–D8` Backend + `F-D1–F-D5` Frontend/CSS)
5. **[04_LIVE_ROUTER_TEST_REPORT.md](file:///c:/Users/internet/Desktop/xkeen%20route/xkeen-ui-ext-main/docs/DEEP_AUDIT_2026-11-19/04_LIVE_ROUTER_TEST_REPORT.md)** — протокол и доказательства испытаний `T00–T12` на `192.168.1.1`
6. **[05_LEGACY_AUDIT_RECONCILIATION.md](file:///c:/Users/internet/Desktop/xkeen%20route/xkeen-ui-ext-main/docs/DEEP_AUDIT_2026-11-19/05_LEGACY_AUDIT_RECONCILIATION.md)** — сверка `REWORK_FIRST.md`, `BUG_AUDIT.md` (`141`), `FIX_LEDGER.md` (`95`), `MASTER_AUDIT_REGISTRY.md` (`61`), `КОСЯКИ.md` (`11`) и `7` файлов working tree
7. **[06_REMEDIATION_ROADMAP.md](file:///c:/Users/internet/Desktop/xkeen%20route/xkeen-ui-ext-main/docs/DEEP_AUDIT_2026-11-19/06_REMEDIATION_ROADMAP.md)** — поэтапная дорожная карта исправлений (Волны `0–4`) и требования к анти-регрессионным тестам
