# Отчёт по живому тестированию на тестовом роутере `192.168.1.1` (Треки `T00–T12`)

## 1. Паспорт стенда и условия испытаний

| Параметр | Фактическое значение на стенде |
|---|---|
| **Целевой роутер** | Только **`192.168.1.1`** (подключение по LAN Ethernet; основной `192.168.2.1` не затрагивался ни единым запросом) |
| **Модель / KeeneticOS** | `WBR3000UAX (KN-4110)`, ARM64, RAM `512` МБ (KeeneticOS `5.0.11`), диск `/opt` (`/dev/sdb2`, `8.0G`, свободно `7.4G`) |
| **Протестированная сборка** | `xkeen-route v1.17.0-beta.11.0` (`HEAD ac0368e`, порт `:1001`, Mihomo `v1.19.33`, `iptables v1.4.21`) |
| **Сетевой профиль WAN** | Прямой WAN через модем (`apcli0` `192.168.2.52`), но с реальной **блокировкой/деградацией зарубежного прямого трафика («белые списки» WAN)**: `ya.ru`/`vk.ru` дают HTTP `302` (`0.16–0.22s`), тогда как `1.1.1.1:53`, `github.com`, `drt2em65z.timeweb.cam` и `static.adguard.com` по direct-WAN уходят в timeout (`5–15s`) |
| **Отличие `beta.11.0` vs рабочий каталог ПК** | На роутере работает закоммиченный `HEAD` (`v1.17.0-beta.11.0`). Локальные 7 незакоммиченных правок на ПК (`router.rs`, `system_ssh.rs`, `free_sub/mod.rs`, `mihomo.rs`, `validator.rs`, `SystemSettingsCards.tsx`, `КОСЯКИ.md`) в бинарник на роутере не заливались (согласно режиму read-only аудита) |
| **Состояние стенда к финализации отчёта (`14:04+03:00`)** | Связь с **`192.168.1.1`** восстановлена (`Ethernet 1 Gbps`), все треки **`T00–T12` выполнены на 100%**. После перезагрузки роутера в `/opt/etc/init.d/` отсутствует автозапуск `mihomo` (скрипта `S05xkeen` нет; автостартовали только `S99xkeen-route`, `S99xkeen-ui`, `S51dropbear`, `S10cron`). Откат **`T12`** полностью восстановил `/opt/etc` и очистил тестовые цепочки `iptables` / `ip rule`; затем read-only браузерный проход **`T11`** отработал без мутаций роутера |

---

## 2. Сводная матрица живых треков `T00–T12`

| Трек | Область проверки | Вердикт | Ключевые доказанные факты |
|---|---|---|---|
| **T00** | Сброс до чистых настроек + baseline | **DONE (выявлены баги старта)** | Бэкап `xr-20261119-010247-081` создан и сохранён в `/tmp/audit_baseline_backup/` и на ПК. На baseline роутере `whitelist_detector_status` уже застрял в `WhitelistDetected` с ошибкой `"Сервер 'free_wl' не входит ни в одну переключаемую группу"` |
| **T01** | Чистый старт, `service stop/start/restart` | **FAIL (3 бага)** | На чистой конфигурации `watchdog` делает 5 холостых ретраев Zapret (`01:35:59..01:36:59`); `/api/xkeen/service start` падает из-за отсутствующего `S05xkeen`; `restart` падает при попытке reload `:9090` при остановленном Mihomo; Mihomo слушает `7890` и `7892`, тогда как в `config.json` стоит `mixed_port: 7890` |
| **T02** | Платная подписка `Geodema-WL` (`drt2em65z.timeweb.cam`) | **FAIL (6 багов)** | См. § 3.2: UA-полиморфизм (`143KB JSON` vs `37.6KB YAML`), вечный hang `add_provider` >`65–70s` без HTTP-таймаута, скрытие ошибки `force_update_provider`, отсутствие файла `./providers/paid_wl.yaml`, 23 ноды в списке при ошибке переключения (`PROXY`/`🛡️ Whitelist` не получают `use: [paid_wl]`) и `400 proxy not exist` на спидтесте |
| **T03** | Бесплатные подписки `free_wl` / `free_vpn` | **FAIL (4 бага)** | См. § 3.3: `GET /api/free-sub/status` -> `404` vs `200` на `/api/providers/free-sub/status`; при недоступности прямого DNS `1.1.1.1`/`8.8.8.8` все `24` (`wl`) и `44` (`vpn`) L7-кандидатов дают `l7_passed: 0`, пул пуст (`nodes: []`), поиск длится `>55.7s`, `switch-node` и `speedtest` падают с ошибкой пустого пула |
| **T04** | VK-туннель (`csqtt-client`, `179.253.72.27`) | **PARTIAL / FAIL (4 бага)** | См. § 3.4: нативное скачивание `install_core` зависает (>21s) из-за прямого обращения к `github.com`. После PC-загрузки бинарника (`10,793,016 B`, SHA-256 ок) `csq0` поднялся с IP `10.66.67.2/32` и `20/20` потоками; UDP пробил выход наружу (`ipify = 179.253.72.27` за `0.942s`), но при TLS-запросах по туннелю фиксировался `curl (35) TLS unexpected eof`, на TCP — спам `TUNCONF: превышен лимит потоков` + ложный отказ `/api/vkturn/action test` (`5.016s`) при реально работающем `ipify: 200` (`3.392s`); `vkturn` не добавлен в `🛡️ Whitelist` и `🎮 Gaming` |
| **T05** | Белый список (`StrictXKeen`) + Детектор БС | **PARTIAL / FAIL (3 бага)** | Каталог `249` доменов, global BS (`XKEEN_BS` + `1053`/`10809`) и per-device BS (`T05.7b`) применяются, но `exclude` заносит **и MAC, и IP** в `excluded_devices`; `detect-servers` верно находит `paid_wl` (`23`), однако `whitelist-detector/check` даёт **`offline_percent: 100 (23/23)`**, оставаясь в `state: "Normal"`, `wan_offline: false` |
| **T06** | Failover + `Canary Probe` (`7894`) | **FAIL (2 бага)** | `Canary Probe` на порту `7894` создаётся (`T06.3`), но спустя ~20с после `"wan_offline": false` в детекторе БС `POST /api/failover/check` замерзает со статусом `"WAN Down: физический канал провайдера недоступен"` (противоречие оракулов!). Зафиксирован `404` на `/api/logs/recent` |
| **T07 / T07b** | Игровой режим (конфликт БС, MAC-only, ping) | **FAIL (4 бага)** | Конфликт при активном БС сработал (`0.024s`), отказ MAC-only без IP воспроизведён (`4.073s`, `[B4-006]`), все 8 игровых хостов на `/api/gaming/ping` таймаутятся на `2500ms`, `ignore-conn` CRUD работает, но после ручного выключения БС игровой режим всё равно блокируется, пока активен `📞 VK-туннель [БС]`, а `/api/gaming/status` сообщает ложный `active_server: "Fastest"` при `tunnel_status.reachable: false` |
| **T08 / T08b** | AdGuard Home (`5353` vs `5354`, фаервол) | **CRITICAL FAIL (P0!)** | См. § 3.5: нативный `install` падает (`static.adguard.com` timeout + `Segmentation fault`) с зомби-процессом `30795` (`process_alive: true` после `killall`); на роутере UDP `5353` уже занят системным `avahi-daemon`; на `5354` `AdGuardHome v0.107.80` работает (27 DNS-запросов, `query-log`, `rewrites` CRUD), но `/api/adguard/sync-firewall` возвращает `success: true` при **полностью пустом `iptables -t nat -S` (0 правил `XKEEN_ADGUARD`)** из-за флага `iptables -w 2` |
| **T09 / T09b** | Zapret 2 (`nfqws2`, `S51zapret`, Blockcheck) | **CRITICAL FAIL** | Нативный `install` и `set_preset` зависают (`>25s`); после PC-staging запуск падает с синтаксической ошибкой shell: `/opt/etc/init.d/S51zapret: line 53: syntax error: unexpected word (expecting ")")`; `POST /api/zapret/blockcheck` возвращает фиктивные оценки `23..37` и `is_best: true` при `youtube_ok: false, discord_ok: false` у всех 10 стратегий (`16.059s`); `community-hostlist/sync` падает на `github.com` |
| **T10 / T10b** | Домены/CDN, Antigravity, Бэкапы, Трафик | **PARTIAL / FAIL (5 багов)** | `POST /api/domains` (`10+2` доменов) длится **`7.941s`** (нарушение `<1s` Instant Save), `scan-cdn` таймаут `>25s`, `/api/geo/info` -> `404`; Antigravity поднимает `:53129` + `INPUT ! -i br+ DROP` и чисто снимает при выключении, `/api/antigravity/check` -> `>20s` timeout; `.xkbak` экспортируется как незашифрованный JSON (`19,231 B`); `/api/devices/traffic` показал `download_total: 1126, upload_total: 151384` |
| **T11** | Браузерный E2E аудит (`browser` subagent `048fba0b`) | **DONE (10+ UI-дефектов)** | Проверено `http://192.168.1.1:1001/` (`1280x800` Dark/Light + `390x844` Mobile, без мутаций роутера). Подтверждены: незакрываемая модалка `Редактор конфигов`, сжатые поля `input[type=number]` Failover, инверсия цветов `Прямой`/`VPS` в Light-теме, overflow таблицы `Устройства`, противоречия статусов (`Failover`, `Google AI`, `Zapret`, `AdGuard`) и раздутие шапки на мобильном (`339.6px`) |
| **T12** | Автоматический откат роутера к baseline | **PASS (100% Parity)** | `t12_rollback.py` (`T12_rollback_verify.log`): залито `153,911 B` эталона `/opt/etc`, удалены все артефакты тестов (`AdGuardHome`, `nfqws2`, `csqtt-client`, `paid_*.yaml`), очищены `XKEEN_*`/`GAME` iptables и `ip rule` `40/45/222`; `/api/status` восстановлен за `0.11s` с 100% совпадением всех полей с baseline |

---

## 3. Подробные доказательства по критическим отказам

### 3.1. `T00–T01`: Стартовые ретраи Zapret и управление службой XKeen
- В `T00_clean_reset.log` и `T01_clean_and_service.log:29-41` на «чисто сброшенной» панели, где `zapret.enabled: false` (но по умолчанию включён `gaming.zapret.enabled: true`), сторож каждые 12 секунд 5 раз подряд логирует дублирующееся предупреждение:  
  `[WARN ] [WATCHDOG] ⚠️ Служба Zapret включена, но nfqws не активен! Автоматический запуск службы...` -> `❌ Не удалось запустить службу Zapret после 5 попыток`.
- Из-за бага двойной записи логов (`[B1-012]`) каждая строка WARN записалась в `xkeen-route.log` **дважды** (`T01:29-42`).
- `POST /api/xkeen/service {"action":"stop"}` (`1.374s`) выдал `code: 0` с stderr `sh: can't open '/opt/etc/init.d/S05xkeen': No such file or directory`. Последующий `action: "start"` (`4.05s`) вернул `success: false`, а `action: "restart"` (`1.93s`) упал при попытке дёрнуть остановленный `http://127.0.0.1:9090/configs?force=true` (`T01:57, 102`).
- При запуске Mihomo бинарник слушает порт `7892` (`mixed-port`), а также `7890`/`7891` (`T01:111-114`), хотя `GET /api/settings` отдаёт `mihomo.mixed_port: 7890` (`T01:11`).

### 3.2. `T02`: Платная подписка `Geodema-WL` (`https://drt2em65z.timeweb.cam/...`)
1. **Полиморфизм ответа сервера по User-Agent:** запрос без clash-UA вернул V2Ray JSON (`143,521 B`), а с Clash UA — YAML (`37,610 B`, заголовок `Subscription-Userinfo: upload=0; download=530350776195; total=944892805120; expire=1829858058`).
2. **Блокировка `add_provider` на БС WAN (>65–70s timeout):** из-за отсутствия HTTP-таймаута и прямого обращения `mihomo`/`updater` к `timeweb.cam` по заблокированному direct-каналу все прямые попытки `POST /api/providers/add` зависали более `65s` / `70s` без создания файла (`T02_paid_complete.log:69-202`).
3. **Успешная доставка через SSH reverse tunnel (`127.0.0.1:18888`) вскрыла `[LIVE-T02-NO-PROXY-GROUP-BIND]`:** подписка `paid_wl` добавилась (`4.149s`) и обновилась (`0.039s`, загружено `23` узла с квотой `Geodema-WL`), но:
   - Локальный кеш-файл `/opt/etc/mihomo/providers/paid_wl.yaml` на диске **не появился** (`T02_loopback_paid.log:238`).
   - `POST /api/servers/switch` на узел `Silver-ne_cert_relay` вернул ошибку (`0.016s`): **`"Сервер 'Silver-ne_cert_relay' не входит ни в одну переключаемую группу"`**, потому что `add_provider_to_yaml_full` не добавил `paid_wl` в `use:` групп `PROXY` и `🛡️ Whitelist` (`[B2-006]`).
   - `POST /api/servers/speedtest` упал с `Mihomo /proxies/%F0%9F%9B%A1%EF%B8%8F%20Canary%20Probe: статус 400 Bad Request ("Selector update error: proxy not exist")` (`T02_loopback_paid.log:442`).
4. **Таймаут `SWITCH DIRECT` и дубль лейбла KeeneticOS:** `POST /api/servers/switch` с `server_id: "DIRECT"` завис на `20.051s` (`T02_followup.log:1-2`); `GET /api/diagnostics/health` вернул `"KeeneticOS KeeneticOS 5.0.11"` (`T02_paid_sub.log:1518`).

### 3.3. `T03`: Бесплатные подписки (`free_wl` / `free_vpn`)
- Несогласованность маршрута: `GET /api/free-sub/status` -> `404 API endpoint not found`, рабочий путь — `/api/providers/free-sub/status`.
- При запуске `connect` (`mode: "whitelist"`) с чистым пулом: этап `sources` длился `17.25s`, `prefilter` — `~4s`, а фаза `l7` («Точечная L7-проверка (24 кандидатов)…» для WL / `44` для VPN) выполнялась `>33s` до конца `55.7s` опроса и на `6c` завершилась ошибкой:  
  `"Ни один узел не прошёл L7-проверку. Предыдущая конфигурация сохранена без изменений."` (`T03_free_sub.log:253`).
- Причина провала `l7_passed: 0` при `tcp_passed: 23` (для WL): внутри Mihomo DNS (`1.1.1.1:53` / `8.8.8.8:53`) недоступен на прямом БС WAN (`read udp 192.168.2.52->1.1.1.1:53: i/o timeout`, `T02_followup.log:88`), из-за чего доменные URL `gstatic.com`/`cp.cloudflare.com` не резолвятся внутри L7-пробы (`[B2-004]`).

### 3.4. `T04`: VK-туннель (`csqtt-client` `csqtt-core-0e90df2`)
- **Установка на роутере заблокирована:** `POST /api/vkturn/action {"action":"install_core"}` бесконечно качает `0` байт (>21s) на `github.com` и падает с `"Не удалось скачать (direct): сервер не ответил за 30 с"` (`T04_vkturn.log:12-26`, `T02_paid_sub.log:200`).
- **Работа после PC-staging (`/opt/sbin/csqtt-client`, `10,793,016 B`):**
  - Интерфейс `csq0` с IP `10.66.67.2/32`, маршрутом в таблице `222` и `ip rule` `40`/`45`/`222` поднялся за `4–6s` (`20/20` потоков, `dtls_ok: 40` по UDP).
  - **UDP-режим:** реальный проброс работает (`curl --interface csq0 https://api.ipify.org` -> **`179.253.72.27`**, HTTP `200`, `0.942s`, и заголовок QRATOR с `timeweb.cam` `3496 B` HTTP `200` в `T02_net_diag.log:284-301`).
  - **TCP-режим и ложный негатив healthcheck:** в логах потока массовые предупреждения `[ВОРКЕР #21..#40] Нефатальная ошибка разбора TUNCONF: превышен лимит потоков` и `GETCONF timeout после 4 попыток`. Хотя в TCP-режиме `curl --interface csq0 http://api.ipify.org` реально вернул `179.253.72.27` (`200` за `3.392s`), ручка `POST /api/vkturn/action {"action":"test"}` за `5.016s` вернула ошибку **`"Узел VK-туннеля недоступен или не отвечает на проверку"`** (`T04_vkturn.log:91-92, 139-140`).
  - **Обрыв TLS через `csqtt` / MTU:** HTTPS-запросы через Mihomo `7892 -> vkturn (csq0 MTU 900/1360)` регулярно обрывались на TLS handshake: `curl: (35) TLS connect error: error:0A000126:SSL routines::unexpected eof while reading` (`T02_paid_sub.log:440`, `T10b`).
  - **Конфликт прокси-групп:** добавленный провайдер `vkturn` попал в `PROXY`, `🛡️ Whitelist`, `🛡️ Canary Probe` (`T02_paid_sub.log:949-977`), однако `commit_vkturn` использует неполный `add_provider_to_proxy_groups` (`[B5-012]`).

### 3.5. `T08 / T08b`: AdGuard Home — P0 поломка фаервола на KeeneticOS 5.0.11
1. Нативный `POST /api/adguard/install`: `curl` падает с `Connection timed out after 15000 milliseconds` + **`Segmentation fault`** при загрузке с `static.adguard.com`, затем вешается на зеркале `github.com` и оставляет висеть дочерний процесс (`PID 30795`).
2. Ложный детектор живого процесса в `/api/adguard/health`: даже после `killall AdGuardHome` health-чек возвращает **`process_alive: true`** (`api_ok: false`, `T08_T09.log:391`), так как поиск по подстроке `"AdGuardHome"` цепляет зависшую команду `sh -c ...` от установщика (`[B1-014]` аналогично `matches_target_needle`).
3. Конфликт дефолтного порта `5353`: на `WBR3000UAX` (KeeneticOS `5.0.11`) UDP-порт `5353` **уже занят системным `843/avahi-daemon`** (`T08_T09.log:270`), поэтому стандартный старт AGH без автоперехода на `5354` обречён на провал DNS-слушателя.
4. **Доказанный P0-баг синка `iptables` (`T08b.3`):** на порту `5354` сам `AdGuardHome v0.107.80` полностью работоспособен (обработал `27` DNS-запросов, `query-log` и `rewrites` CRUD отвечают `200 OK`). Однако `POST /api/adguard/sync-firewall` вернул:
   - `{"data":{"dns_port":5354,"message":"DNS-редирект 53 -> 5354 переприменён"},"success":true}`
   - а немедленная проверка по SSH (`iptables -t nat -S`, `T08b_T09b.log:512`) показала **0 правил и отсутствие цепочки `XKEEN_ADGUARD`**!
   - Первопричина в коде `/opt/etc/ndm/netfilter.d/040-adguard.sh` (выведен в `T08b_T09b.log:512`): каждая команда вызывает `iptables -w 2 ... 2>/dev/null || true`, тогда как встроенный в роутер `iptables v1.4.21` отвергает числовой аргумент к `-w` (`Bad argument '2'`, подтверждено в `T02_net_diag.log:194`). Весь DNS LAN продолжает уходить в обход AdGuard (`0% blocked`).

### 3.6. `T09 / T09b`: Zapret 2 (`S51zapret: line 53`, фейковый `blockcheck`)
- Нативный `POST /api/zapret/action {"action":"install"}` и `set_preset` ушли в таймаут `>25s`.
- После доставки `nfqws2` (`364,808 B`) с ПК вызов `start` провалился с синтаксической ошибкой сгенерированного init-скрипта:  
  `"/opt/etc/init.d/S51zapret: line 53: syntax error: unexpected word (expecting \")\")"` (`T08_T09.log:621`).
- `POST /api/zapret/blockcheck` за `16.059s` при **нулевой доступности всех проверяемых ресурсов** (`youtube_ok: false`, `youtube_cdn_ok: false`, `discord_ok: false`, `discord_cdn_ok: false`, `tls_handshake_ms: 0`) присвоил всем 10 стратегиям баллы **`score: 23..37`** и объявил `multisplit` победителем (`is_best: true`, `best_strategy_id: "multisplit"`, `T08_T09.log:633-820`), подтверждая `[B6-004]`.

### 3.7. `T11`: Браузерный E2E-аудит (`http://192.168.1.1:1001/`, read-only, `1280x800` + `390x844`)

*(Верифицировано через `BROWSER_HOST_FACTS` для субагента `048fba0b-a6ec-44af-aa9d-529ec519f143`: `visited_origins: http://192.168.1.1:1001`, `blocked_origins: none`, `latest_screenshot: final_step_93.png`, а также прямым визуальным просмотром кадров Dark/Light/Mobile и проверкой DOM-замеров из транскрипта без каких-либо мутаций настроек на роутере. Примечание: в ранней части трассировки субагента клики по устаревшим HUD `ref`-идентификаторам шапки вызывали случайное нажатие на соседнюю кнопку `Zapret 2`, тогда как прямой клик по селектору переключения темы `title*="светлую тему"` переключил тему штатно без редиректа.)*

Подтверждённые живые дефекты UI / CSS / UX на чистом стенде (`v1.17.0-beta.11.0`):

1. **Незакрываемая модалка «Редактор конфигов» (`#settings`):** после открытия редактора через CSS-селекторы `.editor-backdrop` не закрывалась ни по `Escape` (`stillOpenAfterEsc: true`), ни по кнопке `"Отмена"` (`stillOpenAfterCancel: true`), ни по `"✕"` (`title="Закрыть"`, `stillOpen: true`), ни по клику по backdrop `(20, 400)` — для выхода требовалась перезагрузка страницы.
2. **Схлопнутые числовые поля Failover (`#settings`):** три поля `<input type="number">` (`Порог пинга для смены узла, мс`, `Интервал проверки пинга, сек`, `Тест возврата, сек`) сжимаются в узкие вертикальные «пилюли», полностью скрывая свои числа (`300`, `60`, `180`) и упираясь в правую границу карточки.
3. **Инверсия цветов каналов трафика в Светлой теме (`#dashboard`, `html[data-theme="light"]`):** плашка `🟢 Прямой (оценка)` рисуется с **синим** бордером и синим текстом, а плашка `🔵 VPS` — с **зелёным** бордером и зелёным текстом.
4. **Низкий контраст в Светлой теме (`#servers`, `final_step_93.png`, и модалка Zapret):** кнопка подключения `🔌` залита ярко-синим при чёрной иконке вилки; бейдж `🟣 Белый список` имеет бледный текст на светло-сиреневом фоне; карточка `Бесплатный обход БС (А...` обрезается многоточием и переносом пилюли на 2-ю строку сдвигает нижнюю панель действий относительно `FREE-VPN`; красные столбики сигнала рисуются рядом с зелёным `Flow OK` и `—`; в светлой модалке `Управление ядром Zapret (DPI)` карточка активного ядра остаётся тёмно-угольной внутри белого окна.
5. **Горизонтальный скролл и смешанная локализация таблицы «Устройства» (`#devices`, `1280x800`):** таблица не помещается в ширину и срезает правые кнопки действий (`Failover`, `Расписание`); обрезаются значения `"DESK..."`, `"Политика п..."`, `"Общий активный ..."`; смешаны RU/EN (`Search devices...`, `Online 1 / Offline 0 / All 1`, `Refresh`, `1 online / 0 offline out of 1 devices`, `No offline devices`, `Gaming OFF`); кнопка `"🗺️ Карта политик"` по клику не сменила вид.
6. **Противоречия статусов и вёрстки по вкладкам (`#dashboard`, `#gaming`, `#ai`, `#adguard`, `#zapret`, `#rules`, `#diagnostics`, `#help`):**
   - На `#dashboard` карточка `🛰 Активный сервер` растягивается под высоту соседней `Failover контроль` с пустотой ~200 px, журнал Failover утверждает `"Автоматический мониторинг активен. Все серверы работают в штатном режиме."` при выключенном тумблере Failover, а при перезагрузке мигают заглушки (`Проверка...`, `Mihomo —`, `Zapret 1 —`, `🔴 Остановлен`, `Keenetic KeeneticOS`).
   - В `#gaming` тост `"❌ Ошибка API"` накладывается на нижнюю липкую панель `"💾 Сохранить и применить"`, KPI показывают `"✕ Проверка..."` и `"TCP: Сбой / UDP: Сбой"`.
   - В `#ai` (Google Antigravity) рядом с активным зелёным тумблером и `"Автоматический прозрачный режим активен"` висит пилюля `"⚪ Отключено"`.
   - В `#adguard` при `"⚪ Не установлен"` рендерятся кликабельный контрол `"Защита отключена"`, ссылка `Веб-панель AGH↗` и зелёные пилюли `"Активен"` для upstream `127.0.0.1:1053` / `77.88.8.8`.
   - В `#zapret` главная страница показывает `"🔴 Не установлен"`, но одновременно горят зелёный бейдж `"● LIVE NETFILTER"` и строка модалки `"Текущее активное ядро на роутере: Zapret 2 (nfqws2) v1.0.6"`.
   - Из `#connections` клики по навигации `📋 Правила` и `🩺 Диагностика` оставили `#connections`, тогда как прямой переход по `#rules` / `#diagnostics` работает; в `#rules` не совпадает регистр фильтра (`DOMAIN-SUFFIX`) и таблицы (`DomainSuffix`), в `#diagnostics` дублируется `"KeeneticOS KeeneticOS 5.0.11"`, в `#help` перепутаны цвета трафика (`🔵 Прямой` / `🟢 Проксированный`) и указаны устаревшие названия вкладок (`Маршрутизация`, `Домены`, `🔌 Соединения`).
7. **Мобильная вёрстка (`390x844`):** `<header>` складывается в 5 строк высотой **`339.6px`**, горизонтальная `<nav>` добавляет **`52.8px`** (~416 px = ~49% высоты экрана `844px`, справа обрезается `"🛡️ Бел..."`), легенда трафика разрывает `"Прямой трафик (оценка)"` с висящим `(оценка)`, а на `#servers` карточки серверов начинаются на `y=990px` и `y=1171px` (ниже первого экрана).

---

## 4. `T12`: Итоги автоматического отката к baseline (подтверждено `T12_rollback_verify.log`)

После восстановления соединения с `192.168.1.1` выполнен обновлённый `scratch/live_tests/t12_rollback.py` (загрузка `153,911 B` архива эталонного `/opt/etc` из `scratch/router_baseline/extracted/etc` с сохранением прав `+x` для `init.d` и `ndm/*.sh`, очистка `XKEEN_ADGUARD` + `ip rule pref 40/45/222`, перезапуск `/opt/sbin/mihomo` и `S99xkeen-route`):

| Проверка | Результат | Доказательство (`T12_rollback_verify.log`) |
|---|---|---|
| **T12.0–T12.1** | **PASS (`rc=0`)** | Восстановлены `/opt/etc/{xkeen-route,mihomo,ndm,zapret,init.d}`, удалены `/tmp/audit_baseline_backup` и все временные бинарники/конфиги из `staged_for_rollback.json` |
| **T12.2** | **PASS (`0.11s`)** | `GET :1001/api/status` -> HTTP `200`, `success: true`, `mihomo_available: true`, `service_running: true` |
| **T12.3** | **PASS (100% Parity)** | Версия `v1.17.0-beta.11.0`, `active_server: null`, `adblock_enabled: false`, `whitelist_mode_active: false`, `whitelist_providers: ["free_wl"]`, `gaming: false`, `failover: false`, `zapret installed/running: false/false`, точное совпадение списка 5 бэкапов (`xr-20261119-010247-081`..`xr-20261117-022926-015`) |
| **T12.4–T12.5** | **PASS** | Отсутствуют `/opt/AdGuardHome`, `/opt/bin/AdGuardHome`, `/opt/etc/AdGuardHome`, `/opt/zapret2`, `/opt/sbin/nfqws2`, `/opt/sbin/csqtt-client`, `paid_*.yaml`; в `ps w` запущены только `/opt/sbin/mihomo` (`PID 15768`), `/opt/sbin/xkeen-route -p 1001` (`PID 15779`) и штатные службы (`dropbear`, `xkeen-ui`, `ndm`) |
| **T12.6–T12.7** | **PASS** | В `ip rule` отсутствуют `40`, `45`, `222`, интерфейс `csq0` удалён; в `iptables-save` (`*nat`, `*mangle`, `*filter`) нет ни одной цепочки или правила `XKEEN_*` или `GAME` |
