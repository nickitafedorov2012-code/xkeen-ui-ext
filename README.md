# 🛣️ XKeen Route

Веб-панель управления **раздельной маршрутизацией** для роутеров Keenetic/Netcraze с
[XKeen](https://github.com/jameszeroX/XKeen) (ядра Xray/Mihomo). Работает прямо на роутере
(Entware), интерфейс — в браузере с любого устройства локальной сети.

Порт функциональности десктопного приложения *Keenetic Policy & XKeen Manager* в веб:
форматы данных совместимы (AUTO-DEVICE-блоки в `/opt/etc/mihomo/config.yaml` — общее
хранилище правил с ПК/Android-версиями).

## ✨ Возможности

- [x] Веб-панель (тёмная тема), один бинарник без зависимостей, порт 1001
- [x] Конфиг с deep-merge (`/opt/etc/xkeen-route/config.json`)
- [x] Список серверов Mihomo: протокол, пинг, активный; переключение; приоритетный
- [x] Устройства: политика Keenetic, лимит скорости, персональный сервер Mihomo
- [x] Раздельная маршрутизация per-device (AUTO-DEVICE-блоки, без SSH, merge-семантика)
- [x] Автоматический failover (порог пинга, автовозврат на приоритетный)
- [x] Дашборд: статус роутера/Mihomo + лента событий failover

## ⚡ Установка (Entware)

Стабильная/Latest версия:

```sh
curl -Ls https://raw.githubusercontent.com/nickitafedorov2012-code/xkeen-ui-ext/main/setup.sh | sh
```

Бета:

```sh
curl -Ls https://raw.githubusercontent.com/nickitafedorov2012-code/xkeen-ui-ext/main/setup.sh | sh -s -- beta
```

Удаление (конфиги сохраняются):

```sh
curl -Ls https://raw.githubusercontent.com/nickitafedorov2012-code/xkeen-ui-ext/main/setup.sh | sh -s -- uninstall
```

Удаление полностью (вместе с конфигами):

```sh
curl -Ls https://raw.githubusercontent.com/nickitafedorov2012-code/xkeen-ui-ext/main/setup.sh | sh -s -- uninstall purge
```

При запуске с терминала (SSH) скрипт показывает меню: установить/обновить, удалить или удалить полностью.

> **Если raw.githubusercontent.com недоступен** (блокировки на некоторых сетях) — используйте зеркало:
>
> ```sh
> curl -Ls https://ghproxy.net/https://raw.githubusercontent.com/nickitafedorov2012-code/xkeen-ui-ext/main/setup.sh | sh
> ```

Порт по умолчанию: **1001**. Управление сервисом:

```sh
sh /opt/etc/init.d/S99xkeen-route start|stop|restart|status
```

## 🔧 Сборка из исходников

Фронтенд (Node.js 18+):

```sh
cd frontend && npm install && npm run build
```

Бэкенд (Rust 1.75+; для целевых устройств — musl):

```sh
cd backend && cargo build --release        # dev-сборка под текущую ОС
```

Кросс-сборка под роутер — через [cross](https://github.com/cross-rs/cross) или
GitHub Actions (workflow `build.yml`: mips / mipsel / arm64 musl).

### Локальная разработка

```sh
# терминал 1: бэкенд (порт 1001)
cd backend && cargo run
# терминал 2: фронтенд с hot-reload (прокси /api -> :1001)
cd frontend && npm run dev
```

## ⚙️ Конфигурация

`/opt/etc/xkeen-route/config.json` (может быть частичным, остальное — из дефолтов):

```json
{
  "rci":     { "host": "127.0.0.1", "port": 79, "login": "admin", "password": "", "token": "" },
  "mihomo":  { "host": "127.0.0.1", "port": 9090, "secret": "" },
  "failover": { "enabled": false, "ping_threshold_ms": 300, "priority_server": "",
                "auto_restore_priority": true, "interval_secs": 60 },
  "refresh_interval_sec": 10
}
```

RCI-доступ: приоритет у `token` (`X-Ndma-Tkn`; панель подхватывает токен из
`/opt/etc/xkeen/xkeen.json`, как XKeen-UI), иначе — challenge-auth по login/password.

## ⚠️ Безопасность

Панель **не имеет авторизации** (решение осознанное) и предназначена **только для
локальной сети**. Не открывайте порт наружу без VPN/KeenDNS с паролем.

## 🙏 Благодарности

- [zxc-rv/XKeen-UI](https://github.com/zxc-rv/XKeen-UI) — источник архитектурных идей
- [Skrill0/XKeen](https://github.com/Skrill0/XKeen), [jameszeroX/XKeen](https://github.com/jameszeroX/XKeen)
- [Anonym-tsk/nfqws-keenetic](https://github.com/Anonym-tsk/nfqws-keenetic)

## 🧠 Грабли и неочевидные знания (выстрадано отладкой)

Всё ниже — реальные грабли, на которые наступили при разработке. Прежде чем менять
код, работающий с Mihomo/YAML/кодировками — перечитай этот раздел.

### Mihomo REST API (127.0.0.1:9090)

- **`GET /proxies` НЕ содержит прокси из провайдеров!** Только то, что объявлено в
  секции `proxies:` конфига, плюс группы и служебные (DIRECT/REJECT/...). Подписка
  на 100+ серверов через `proxy-providers` в `/proxies` не видна вообще — при этом
  `all`-список группы Fastest их имена содержит. Полный список провайдерных серверов
  живёт в отдельном эндпоинте **`GET /providers/proxies`** (структура:
  `{"providers": {"geodema": {"proxies": [...]}, ...}}`). Панель, показывающая «3
  сервера», — именно поэтому: `get_servers` в `mihomo.rs` читает только `/proxies`,
  надо мержить оба эндпоинта.
- Типы в API — **CamelCase**: `URLTest`, `Fallback`, `Selector`, `Vless`,
  `Shadowsocks` (не `url-test`/`vless`, как в YAML).
- **Reload**: `PUT /configs?force=true` с телом `{"path": "/opt/etc/mihomo/config.yaml"}`.
  Пустой ответ (204) = успех. `"Body invalid"` = сломанный JSON в теле (не в конфиге!).
- Принудительное обновление провайдера: `PUT /providers/proxies/:name` (пустой ответ
  = ок). `exclude-filter` провайдера применяется при его загрузке.
- mihomo запущен без аргументов → конфиг берёт по умолчанию из своей рабочей папки
  (`/opt/etc/mihomo/config.yaml`). Других конфигов нет.
- Версия ядра: 1.19.2x (User-Agent в провайдерах `ClashMeta/1.19.24`).

### Поведение фильтров (проверено на 1.19.2x)

- **Групповой `exclude-filter` НЕ фильтрует провайдеров, подключённых через явный
  `use:`** — только тех, кто подтянут через `include-all: true`. Это ломает «очевидное»
  решение игнора. Рабочий путь: дописывать игнор-подстроки в **собственный
  `exclude-filter` провайдера** (XKeen сам так исключает RU/Hysteria).
- **Пустой `proxies:` в группе = невалидный YAML** → reload отвечает 400, конфиг не
  применяется (при этом старый конфиг продолжает работать — ошибка не фатальна, но
  и не видна). Никогда не вставляй пустой список: `if !kept.is_empty()`.
- Имена провайдерных серверов **дублируют статические**: «Финляндия» существует и в
  секции `proxies:`, и в подписке geodema. Статических серверов в XKeen-конфиге
  обычно 1–2, вся подписка (100+) — через провайдеров.

### YAML-грабли

- **`key:# comment` (без пробела перед `#`) — невалидный YAML**: `#` становится частью
  ключа → `could not find expected ':'`. Вставка маркер-блоков (AUTO-DEVICE) после
  `proxy-groups:`/`rules:` обязана начинаться с `\n`. Ровно это ломал `apply_routing`.
- При построчном переписывании секций групп: проверка «начала новой целевой группы»
  (`- name: Fastest`) должна **сначала сбросить хвост предыдущей** (дописать
  exclude-filter/proxies/include-all), иначе при переходе Fallback→Fastest хвост
  теряется (между группами пустая строка — она не граница секции!).

### XKeen

- XKeen может **перегенерировать `config.yaml`** (в т.ч. при `S05xkeen restart`) —
  наши правки пережили перегенерацию, но watchdog-восстановления ещё нет. После
  рестарта XKeen проверяй, на месте ли правки.
- AUTO-DEVICE-блоки: селектор-группа на устройство (имя кодирует IP:
  `Big PC 192_168_2_118` → `ip_from_group_name` парсит подчёркивания) + правило
  `SRC-IP-CIDR` в `rules:`.

### Кодировки: Windows-машина → роутер (главная боль)

- **Кириллица в командной строке plink/PowerShell ломается** (cp866/cp1251
  двойное перекодирование): POST-тело с «Финляндия» превращается в mojibake
  `╨д╨╕╨╜╨╗╤П╨╜╨┤╨╕╤П`, и эта строка сохраняется в конфиг/игнор-лист как есть.
  Правила:
  - POST/JSON с кириллицей — **только через файл**: `curl --data-binary @file`
    (файл писать `[IO.File]::WriteAllText(..., UTF8Encoding($false))`).
  - Файлы на роутер — **только base64** (`base64 -d`), не inline-текст: busybox
    `printf` ломает `\"`, sed через plink ломает кавычки.
  - Даже `[char]0x0424` в PS-команде может исказиться при передаче — проверяй
    байты результата (`[IO.File]::ReadAllBytes` → hex), а не `Get-Content`.
- **Вывод plink → PowerShell-пайплайн декодируется как cp866**: любой UTF-8 дамп
  через пайплайн выглядит как mojibake. Это артефакт отображения, но он же делает
  такие дампы непригодными для суждений о кодировке. Достоверно — только base64
  round-trip или hex байтов.
- Кириллические маркеры поиска в PS-однострочниках (`IndexOf('/// Экранирование...')`)
  искажаются → для правок кода скриптами используй **латинские маркеры**
  (`pub fn ...`, `#[cfg(test)]`).
- Из-за этого в коде появлялись литеральные `$1` и обрезанные строки — после
  скриптовых правок всегда `cargo test` + просмотр места правки.

### Деплой-пайплайн (отработан, порядок обязателен)

1. `cargo test` локально (PATH: `%USERPROFILE%\.cargo\bin;%USERPROFILE%\mingw64\bin`;
   воркспейс на диске `X:` через `subst` — пробелы в пути ломают GNU ld).
2. commit → tag `v*` → push → CI (собирает **только aarch64**) → поллер
   `%TEMP%\xr-poll-ci.ps1` (править имя тега и пути при новом релизе!), ~5–8 мин.
3. Скачать релиз → на роутер через base64+plink (hostkey
   `SHA256:JYD/KYLTxqXSwi7OBY8wkXTO9Cm/8Afrhg88MqfWPIk`, пароль `keenetic`, порт 222).
4. **Порядок: stop → замена бинарника → start** (иначе ETXTBSY).
5. MD5-сверка локального и `/opt/sbin/xkeen-route`.

### Игнор-лист (текущая реализация, v0.2.3)

- Матч — подстрочный, без учёта регистра (`name.to_lowercase().contains(ig)`).
- Статические: `include-all` → явный `proxies:` минус игнорируемые; kept пуст →
  блок не вставлять (см. грабли YAML).
- Провайдеры: игнор-подстроки дописываются в их `exclude-filter`; оригиналы — в
  `config.json` → `provider_filters` (BTreeMap), восстанавливаются при очистке.
- Открытый вопрос: провайдерная «Финляндия» в тесте не исключилась — либо mojibake
  в сохранённой строке (сломанный POST), либо mihomo применяет `exclude-filter`
  провайдера только при HTTP-обновлении (лечится `PUT /providers/proxies/:name`
  или чисткой кэша провайдера). Чистый ретест: POST через
  `%TEMP%\xr-ig6.json` (байты проверены — корректная UTF-8 «Финляндия»).

### Планируемые доработки (не сделано)

- ~~**Полный список серверов в панели**~~ — сделано в v0.2.4 (мерж `/proxies` + `/providers/proxies`).
- ~~**Единый вид имён**~~ — сделано в v0.2.4 (display-name без эмодзи/флагов + mojibake-ремонт;
  `id`/переключение — всегда по оригинальному имени).
- ~~Починить mojibake-имя статического сервера~~ — сделано в v0.2.4 (кнопка
  «Исправить имена» → `POST /api/servers/fix-names`: обратная перекодировка
  cp1251/cp1252/cp866, глобальная замена в config.yaml, reload).
- Watchdog-восстановление правок после перегенерации конфига XKeen'ом.

## 📋 Ченжлог

### v0.3.0 — сервис XKeen (старт/стоп), бэкапы, установка одной командой

- **feat**: управление сервисом XKeen (Настройки → 🖥 Сервис XKeen): Старт / Стоп /
  Рестарт / Статус. Рестарт перегенерирует config.yaml — настройки возвращаются
  к исходным (до любых изменений из панели). Путь init-скрипта настраивается
  (`system.xkeen_init`, по умолчанию `/opt/etc/init.d/S05xkeen`). API: `POST /api/xkeen/service`.
- **feat**: бэкапы как в XKeen-UI (Настройки → 💾 Бэкапы): создание снимка
  (config.yaml Mihomo + config.json панели), список, восстановление (с reload
  Mihomo и перечитыванием конфига панели), удаление. Каталог `{backup_dir}/xkeen-route`
  (по умолчанию `/opt/backups`). API: `GET/POST /api/backups`, `POST /api/backups/restore|delete`.
- **feat**: установка одной командой через SSH:
  `curl -Ls https://raw.githubusercontent.com/.../setup.sh | sh` (+ `beta`, `uninstall`,
  интерактивное меню при запуске с терминала).
- **fix**: tokio-фича `process`, мелкие исправления компиляции.

### v0.2.9 — универсальность + доменные списки (DIRECT / PROXY)

- **refactor (универсальность)**: убраны привязки к конкретному железу:
  - группы устройств больше не хардкодят `use: [geodema, geodema2]` — провайдеры
    задаются в Настройках (`mihomo.device_providers`) или берутся автоматически
    из `proxy-providers:` config.yaml;
  - `switch_server` обнаруживает select-группы динамически (не только PROXY/GLOBAL);
  - `resolve_active_leaf`/`get_servers` — fallback на первый selector;
  - reload Mihomo использует путь конфига из настроек.
- **feat**: доменные списки (Настройки → 🌐 Домены), два окна:
  - «Напрямую» → `DOMAIN-SUFFIX,x,DIRECT` (мимо прокси);
  - «Принудительно через прокси» → `DOMAIN-SUFFIX,x,PROXY`.
  Вставляются AUTO-блоками в начало `rules:` (приоритет над остальными правилами),
  merge-семантика, нормализация (без протоколов/путей, lowercase). API: `GET/POST /api/domains`.
- **tests**: +4 (group_yaml без провайдеров, parse_provider_names, sanitize_domains,
  доменные правила вставка/дубли/очистка). Итого 28/28.

### v0.2.8 — вёрстка таблицы устройств

### v0.2.7 — пинг в дропдауне устройств + per-device failover (резервы и порог пинга)

- **feat**: пинг в списке серверов на странице «Устройства» («Германия · 58 мс»).
- **feat**: per-device цепочки серверов (⚙ у дропдауна): основной + резервы с
  сортировкой, порог пинга (мс) и автовозврат на основной. Хранится в
  `config.json` → `device_routing` (ключ — IP).
- **feat**: фоновый мониторинг `failover.device_failover_enabled` (тумблер
  «⚡ failover устройств»): отвалился или пинг > порога → переключение группы
  устройства на следующий живой из цепочки; восстановился основной → возврат.
  Ручное переключение на сервер вне цепочки не трогается. События — в ленту failover.
- **api**: `GET/POST /api/device-routing`.

### v0.2.6 — починка ремонта имён с флагами-эмодзи

- **fix**: эмодзи-флаг в имени ломал декодирование всей строки, а байт 0xA0 в mojibake
  становится NBSP и рвёт слова при split_whitespace. Порядок теперь: вся строка →
  срезать ведущие флаги и декодировать снова → запасной сплит только по обычному пробелу.
- Проверено на роутере: все 110 имён — чистая кириллица («Россия Санкт-Петербург», «Польша», …).

### v0.2.5 — умный ремонт mojibake

- **fix**: `fix_mojibake_smart` — если строка целиком не декодируется (эмодзи мешает),
  ремонт по словам. Используется в display-name, игнор-матчинге и `fix-names`.

### v0.2.4 — все серверы в панели + mojibake-ремонт + починка провайдерного игнора

- **feat**: `get_servers` мержит `GET /proxies` и `GET /providers/proxies` — панель
  видит все 100+ провайдерных серверов, а не только статические.
- **feat**: единый вид имён — display-name чистится от эмодзи/флагов
  (`clean_display_name`), `id` остаётся оригинальным (переключение по точному имени).
- **feat**: mojibake-ремонт имён (`fix_mojibake`: обратная перекодировка
  cp1251/cp1252/cp866, до двух уровней). Чинится отображение, игнор-матчинг
  (в `set_ignore` к списку добавляются починенные варианты) и сами имена в
  config.yaml — новый эндпоинт `POST /api/servers/fix-names` + кнопка
  «🩹 Исправить имена» в панели.
- **fix**: «Финляндия» не исключалась из провайдеров — `exclude-filter` провайдера
  применяется только при его загрузке. После reload `set_ignore` теперь принудительно
  обновляет всех HTTP/File-провайдеров (`PUT /providers/proxies/:name`).
- **tests**: +4 (mojibake cp1251, нормальная кириллица не трогается, чистка эмодзи,
  display_name). Итого 24/24.

### v0.2.3 — провайдерный игнор + починка apply_routing

- **fix**: `apply_routing` приклеивал маркеры `# --- AUTO-DEVICE-*-BEGIN ---` к строкам
  `proxy-groups:` / `rules:` (YAML становился невалидным, reload падал с
  `could not find expected ':'`). Добавлен `\n` перед вставкой блоков.
- **feat**: игнор провайдерных серверов — игнор-подстроки дописываются в
  `exclude-filter` провайдеров (`apply_ignore_to_providers`), т.к. групповой
  `exclude-filter` не действует на явные `use:`. Оригинальные фильтры сохраняются
  в `config.json` (новое поле `provider_filters`) и восстанавливаются при очистке.
- **tests**: +1 (провайдерные фильтры: добавление и восстановление). Итого 20/20.

### v0.2.2 — починка перезаписи групп

- **fix**: пустой `proxies:` в группе (когда все статические серверы в игноре) —
  невалидный YAML, reload падал с 400. Все места вставки защищены `!kept.is_empty()`.
- **fix**: потеря «хвоста» группы (exclude-filter/proxies/include-all) при переходе
  Fallback→Fastest — теперь `is_target_start` сначала закрывает предыдущую группу.
- **tests**: +1 (игнор всех статических → список не вставляется). Итого 19.

### v0.2.1 — новая стратегия игнора

- **fix**: старый `apply_exclude_filter` не работал для статических серверов
  (exclude-filter Mihomo не трогает секцию `proxies:`). Заменён на
  `apply_ignore_to_groups`: `include-all` → явный `proxies:`-список минус
  игнорируемые (подстрочный match без учёта регистра, `regex_escape` без якорей).
- **refactor**: удалены старые `apply_exclude_filter`/`filter_line` и 4 старых
  exclude-теста; `api.rs` переключён на новую функцию.
- **tests**: +5 (замена include-all, замена предыдущего списка, восстановление при
  очистке, regex_escape, parse_static_proxy_names).

### v0.2.0 — рабочая панель

- Панель целиком: дашборд, серверы, устройства, настройки, failover, раздельная
  маршрутизация per-device (AUTO-DEVICE-блоки, merge-семантика).
- Игнор-лист v1: поле `ignore_servers`, API `GET/POST /api/ignore`, модалка в
  UI (подход через групповой exclude-filter — позже признан нерабочим для
  статических серверов, см. v0.2.1).
- 17 тестов.

---

# Заметки для разработки (context dump)

Практические знания об окружении, деплое и граблях. Если правите — правьте здесь же.

## Доступ к роутеру (с ПК разработчика)
- plink/pscp лежат в `%TEMP%` (putty-утилиты).
- Подключение: `plink -batch -P 222 -hostkey 'SHA256:JYD/KYLTxqXSwi7OBY8wkXTO9Cm/8Afrhg88MqfWPIk' -l root -pw keenetic 192.168.2.1`
- Копирование: `pscp -scp -P 222 -hostkey '...' файл root@192.168.2.1:/путь` (**только `-scp`** — SFTP на Entware нет).
- **POST-тела через plink не слать** — plink ломает кавычки/кавычки в JSON. Только через файлы (`pscp` + `curl -d @file`) или base64.
- Сложные команды с кавычками/regex: писать в `%TEMP%\xr-cmd.txt` и запускать `plink ... -m %TEMP%\xr-cmd.txt`.
- Вывод plink в PowerShell часто теряется/ломается (cp866). Надёжный паттерн: скрытый bat → `plink ... > %TEMP%\xr-out.txt 2>&1`, затем `Get-Content`. Запуск: `cmd /c start /min "" "%TEMP%\xr-chk.bat"`, потом чтение файла.
- PowerShell показывает UTF-8 JSON как mojibake — это артефакт консоли, данные корректны (проверять hex/curl).

## Сеть роутера: что доступно, что нет
- `raw.githubusercontent.com` — **заблокирован** с роутера.
- Работающие источники: `cdn.jsdelivr.net` и `fastly.jsdelivr.net` (могут кэшировать `@main` с задержкой — см. ниже), `api.github.com`, `github.com` (release-ассеты — напрямую падают по таймауту, нужны зеркала).
- Зеркала для скачивания бинарников: `https://ghproxy.net/URL`, `https://ghfast.top/URL`, фолбэк `http://ghproxy.net/` (у https-зеркал бывают проблемы с сертификатами для curl без CA-бандла; на Entware CA часто нет вообще → в setup.sh везде `curl -k`).
- **jsDelivr кэширует `@main` агрессивно**: purge (`https://purge.jsdelivr.net/gh/...`) не гарантирует обновление на edge, куда попадает роутер. Надёжный путь — релизить тег и ссылаться на `@vX.Y.Z` (теговые URL отдаются свежими). Установщик резолвит ассет через API `releases` (не `latest` — тег latest может быть не проставлен).

## Деплой (пайплайн)
1. Коммит в `main` → тег `vX.Y.Z` → `git push origin vX.Y.Z`.
2. CI (`.github/workflows/build.yml`): собирает фронтенд, кросс-компилирует `aarch64-unknown-linux-musl` через `cross`, публикует prerelease с ассетом `xkeen-route-arm64-v8a`. Сборка занимает 15–40 мин (раннеры бывают медленные/очередь).
3. Обновление роутера: `curl -Ls https://cdn.jsdelivr.net/gh/nickitafedorov2012-code/xkeen-ui-ext@vX.Y.Z/setup.sh | sh` — установщик сам резолвит последний релиз через API, качает бинарник (github → ghproxy → ghfast → http-фолбэк), создаёт init, рестартует. Конфиг сохраняется.
4. Проверка: `sh /opt/etc/init.d/S99xkeen-route status`, `curl -s http://127.0.0.1:1001/api/status`, хэш ассета в `/` должен смениться.

## Локальная сборка/тесты (Windows ПК)
- cargo есть в `%USERPROFILE%\.cargo\bin` (в PATH PowerShell его НЕТ — звать полным путём или через bat).
- `cargo check` / `cargo test` запускать через скрытый bat с перенаправлением в файл (форграунд-процессы терминал убивает): см. паттерн plink выше.
- npm/node в PATH нет — фронтенд собирает только CI.
- **Перед тегом обязательно `cargo check` локально** — две сборки подряд падали в CI из-за тривиальных ошибок (незакрытая скобка, импорт макросов), а лог CI недоступен без авторизации (ветка `ci-log` с build.log иногда не создаётся).

## Грабли в коде
- **React: все хуки — строго до любого условного return.** Уже дважды ловили React #310 («Rendered more hooks») в Settings.tsx из-за хуков после `if (!settings) return`. ErrorBoundary в App.tsx теперь показывает текст ошибки вместо белого экрана — если пользователь присылает скрин «Ошибка интерфейса», читать сообщение там.
- Макросы `log_i!/log_w!/log_e!` (`#[macro_export]`) в субмодулях требуют `use crate::{log_i, log_e};` (в main.rs — видны без импорта).
- Логи: файл `/opt/etc/xkeen-route/xkeen-route.log`, ротация 2 МБ → `.log.old`, опциональный UDP syslog (`logs.remote_syslog` в конфиге, применяется после рестарта панели). API: `GET /api/logs?lines=N`, `GET /api/logs/download`, `POST /api/logs/clear`.
- Кэш браузера: index.html отдаётся с `Cache-Control: no-store` (middleware `no_cache`), но после обновления пользователю иногда нужен Ctrl+F5.

## Состояние/прочее
- Порт панели: 1001 (`-p`), конфиг: `/opt/etc/xkeen-route/config.json`, бэкапы: `/opt/backups/xkeen-route/`.
- Рестарт XKeen (`S05xkeen restart`) перегенерирует config.yaml и сбрасывает правки панели (домены, per-device, игнор) — это известное ограничение; кандидат в будущие фиксы: watchdog, восстанавливающий AUTO-блоки.
- Идеи бэклога: mips/mipsel CI-сборки, пароль-авторизация (Argon2 как в XKeen-UI), webhook/Telegram-уведомления об ERROR.


