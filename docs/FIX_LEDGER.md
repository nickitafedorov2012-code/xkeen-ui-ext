# Журнал исправлений

Журнал вместе с кодом и проверенными логами отражает прогресс; сам по себе он не доказывает исправление.
После новой сессии или сжатия контекста прочитай его и вернись к незавершённым и разблокированным пунктам.

Статусы:
- `FIXED` — действительный RED или документированный ALREADY, реальный регрессионный тест, проверка исправления и `# GATE OK` в `docs/fix_logs/bug-N-green.log`.
- `NOT_REPRODUCED` — сценарий проверен, с конкретными доказательствами отсутствия заявленной проблемы.
- `NEEDS_ROUTER` — требуется проверка на роутере, независимо от пометки в аудите; указать, что проверено локально.
- `BLOCKED` — конкретное препятствие, недостающая проверка, решение или зависимость и следующий шаг.

Для неприменимых полей записывай причину, не придумывай результат. Подпункты учитывай отдельно, но счётчик номеров 1–141 не увеличивай.

## Сводка

| № | Фаза | Статус | Тест (spec для шлюза) | Файлы | red_kind |
|---|---|---|---|---|---|
| 125 | A | FIXED | be:regression_bug_125 | backend/src/api.rs, backend/src/updater.rs | assert |
| 91 | A | FIXED | be:regression_bug_91 | backend/src/auth.rs | assert |
| 90 | A | FIXED | be:regression_bug_90 | backend/src/main.rs, backend/src/auth.rs | assert |
| 141 | A | FIXED | be:regression_bug_141 | backend/src/api.rs | assert |
| 61 | A | FIXED | be:regression_bug_61 | backend/src/bot.rs | assert |
| 128 | A | FIXED | be:regression_bug_128 | backend/src/bot.rs | assert |
| 44 | A | FIXED | be:regression_bug_44 | backend/src/adguard.rs | assert |
| 89 | A | FIXED | be:regression_bug_89 | backend/src/notifications.rs | assert |
| 92 | A | FIXED | be:regression_bug_92 | backend/src/auth.rs | assert |
| 93 | A | FIXED | be:regression_bug_93 | backend/src/auth.rs, backend/src/main.rs | assert |
| 68 | A | FIXED | be:regression_bug_68 | backend/src/updater.rs | assert |
| 69 | A | BLOCKED | - | backend/src/updater.rs | - |
| 1 | B | FIXED | be:regression_bug_1_adguard | backend/src/adguard.rs, backend/src/api.rs | assert |
| 2 | B | FIXED | be:regression_bug_2_resolve_excluded | backend/src/routing.rs, backend/src/api.rs | assert |
| 3 | B | FIXED | be:regression_bug_3_whitelist_strict | backend/src/routing.rs | assert |
| 70 | B | FIXED | be:regression_bug_70 | backend/src/api.rs, backend/src/adguard.rs, backend/src/direct_traffic.rs | assert |
| 71 | B | FIXED | be:regression_bug_71 | backend/src/direct_traffic.rs | assert |
| 72 | B | FIXED | be:regression_bug_72 | backend/src/traffic.rs | assert |
| 88 | B | FIXED | be:regression_bug_88 | backend/src/notifications.rs | assert |
| 97 | B | FIXED | be:regression_bug_97a / 97b / 97c | backend/src/override_sync.rs | 97a:compile, 97b:assert, 97c:compile |
| 73 | F | FIXED | be:regression_bug_73 | backend/src/traffic.rs | compile |
| 102 | B | FIXED | be:regression_bug_102 | backend/src/failover.rs, backend/src/mihomo.rs | assert |
| 103 | B | FIXED | be:regression_bug_103 | backend/src/failover.rs | compile |
| 114 | B | FIXED | be:regression_bug_114 | backend/src/adguard.rs | assert |
| 116 | B | FIXED | be:regression_bug_116 | backend/src/watchdog.rs, backend/src/config.rs, backend/src/api.rs, backend/src/bot.rs | compile |
| 133 | B | FIXED | fe:src/components/AdGuard.test.tsx::bug 133: / be:regression_bug_133 | frontend/src/components/AdGuard.tsx, frontend/src/components/AdGuard.test.tsx, backend/src/api.rs | 133a:assert, 133b:compile |
| 28 | B | FIXED | be:regression_bug_28 | backend/src/watchdog.rs | compile |
| 31 | B | BLOCKED | be:regression_bug_31a | backend/src/api.rs | - |
| 56 | B | FIXED | be:regression_bug_56 | backend/src/api.rs | assert |
| 57 | B | FIXED | be:regression_bug_57 | backend/src/api.rs, backend/src/watchdog.rs | compile |
| 59 | B | FIXED | be:regression_bug_59a / be:regression_bug_59c | backend/src/bot.rs, backend/src/watchdog.rs, backend/src/main.rs | 59a:assert, 59c:compile |
| 62 | B | FIXED | be:regression_bug_62 | backend/src/failover.rs | compile |
| 27 | B | FIXED | be:regression_bug_27 | backend/src/api.rs | assert |
| 38 | B | BLOCKED | be:regression_bug_38b | backend/src/api.rs | - |
| 109 | B | FIXED | be:regression_bug_109 | backend/src/api.rs | assert |
| 135 | B | FIXED | fe:src/components/Devices.test.tsx::bug 135: / be:regression_bug_135 | frontend/src/components/Devices.tsx, frontend/src/components/Devices.test.tsx, backend/src/api.rs | 135a:assert, 135b:compile |
| 84 | B | FIXED | be:regression_bug_84 | backend/src/speedtest.rs, backend/src/routing.rs | compile |
| 118 | B | FIXED | be:regression_bug_118 | backend/src/watchdog.rs | compile |
| 130 | C | FIXED | be:regression_bug_130a / be:regression_bug_130b | backend/src/config.rs, backend/src/main.rs | 130a:assert, 130b:compile |
| 94 | C | FIXED | be:regression_bug_94 | backend/src/config.rs | assert |
| 76 | C | FIXED | be:regression_bug_76 | backend/src/subscription_parser.rs | assert |
| 77 | C | FIXED | be:regression_bug_77 | backend/src/subscription_parser.rs | assert |
| 80 | C | FIXED | be:regression_bug_80 | backend/src/api.rs | assert |
| 122 | C | FIXED | be:regression_bug_122 | backend/src/mihomo.rs | compile |
| 39 | C | FIXED | be:regression_bug_39_ | backend/src/routing.rs | assert |
| 12 | C | FIXED | be:regression_bug_12_ | backend/src/routing.rs | assert |
| 51 | C | FIXED | be:regression_bug_51_ | backend/src/routing.rs | assert |
| 64 | C | FIXED | be:regression_bug_64 | backend/src/api.rs | compile |
| 41 | B | FIXED | be:regression_bug_41 | backend/src/api.rs | assert |
| 45 | B | BLOCKED | be:regression_bug_45a | backend/src/api.rs, backend/src/routing.rs | - |

## Записи

### Баг 125 — Инъекция команд через настройки (system.xkeen_route_init / xkeen_route_bin)
- Правила перечитаны: 2026-11-05 01:44:33 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/api.rs:658` — `merge_settings_update`, `backend/src/updater.rs:745` — `install_update`
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
  let mut new_cfg: config::AppConfig =
      serde_json::from_value(merged).map_err(|e| format!("Некорректные настройки: {}", e))?;
  new_cfg.auth = current.auth.clone();
  new_cfg.failover.migrate_priority();
  Ok(new_cfg)
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: PUT /api/settings принимал произвольные строки в system.xkeen_route_init и system.xkeen_route_bin. updater.rs форматировал init_script_str прямо в sh -c "(sleep 1 && {} restart)", что давало RCE при перезапуске обновления.
- Первопричина одним предложением: merge_settings_update вырезал только auth и не валидировал пути к бинарникам и init-скриптам через is_valid_init_script_path, а updater.rs не валидировал путь перед запуском sh.
- Тест: `backend/src/api.rs::regression_bug_125_command_injection_in_settings`; spec: `be:regression_bug_125`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): вызывает `merge_settings_update` с инъекцией команд в пути init/bin секции system и проверяет, что возвращается ошибка валидации.
- RED: `docs/fix_logs/bug-125-red.log`, реальная причина падения: `thread 'api::settings_and_service_regression_tests::regression_bug_125_command_injection_in_settings' (6904) panicked at src\api.rs:10911:9: Опасные пути в system должны отклоняться при валидации`
- Исправление (что и почему, 1–3 предложения): в `merge_settings_update` добавлена валидация всех путей к init-скриптам и бинарникам секции system через `is_valid_init_script_path`. В `updater.rs` добавлена контрольная проверка перед запуском команды перезапуска.
- Какие инварианты затрагивает и почему не нарушены: безопасность и валидация путей (/opt/ и whitelist символов); пути по умолчанию соответствуют требованиям.
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Не задеты, штатные пути сервисов (/opt/etc/init.d/S* и /opt/sbin/*) полностью валидны.
- GREEN: docs/fix_logs/bug-125-green.log; строка === GATE OK bug N | diff ... === скопирована из вывода:
  === GATE OK bug 125 | diff 0ac65529856b | лог docs/fix_logs/bug-125-green.log ===
- Статус: FIXED

### Баг 91 — DNS-rebinding: CSRF-фильтр доверяет произвольному Host при совпадении с Origin
- Правила перечитаны: 2026-11-05 01:49:20 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/auth.rs:532` — `is_trusted_origin`
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
  if let Some(host) = host_header {
      let host_trimmed = host.trim();
      let host_only = extract_host_only(host_trimmed);
      if origin_host_part.eq_ignore_ascii_case(host_trimmed) || origin_host_only.eq_ignore_ascii_case(host_only) {
          return true;
      }
  }
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: Злоумышленник настраивает DNS своего домена evil.com на локальный IP роутера. Браузер при межсайтовом запросе отправляет Origin: http://evil.com и Host: evil.com. Ранее `is_trusted_origin` безоговорочно возвращал true, считая запрос доверенным, что приводило к обходу CSRF защиты и RCE/управлению панелью при выключенной auth.
- Первопричина одним предложением: Совпадение Origin с Host проверялось без валидации того, что сам Host является легитимным адресом/доменом роутера.
- Тест: `backend/src/auth.rs::regression_bug_91_dns_rebinding_origin_matches_arbitrary_host`; spec: `be:regression_bug_91`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): вызывает `is_trusted_origin("http://evil.com", Some("evil.com"))` и проверяет, что возвращается false.
- RED: `docs/fix_logs/bug-91-red.log`, реальная причина падения: `thread 'auth::tests::regression_bug_91_dns_rebinding_origin_matches_arbitrary_host' (14464) panicked at src\auth.rs:821:9: assertion failed: !is_trusted_origin("http://evil.com", Some("evil.com"))`
- Исправление (что и почему, 1–3 предложения): Добавлена функция `is_legitimate_host`, проверяющая, что заголовок Host относится к loopback, RFC1918/link-local IP, локальным однословным именам, .lan/.local или официальным доменным зонам Keenetic (my.keenetic.net, *.keenetic.pro и т.д.). В `is_trusted_origin` совпадение с Host принимается только при выполнении `is_legitimate_host`.
- Какие инварианты затрагивает и почему не нарушены: CSRF-защита и валидация хостов; легитимные локальные обращения и KeenDNS продолжают работать.
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Не задеты, маршрутизация и API вызовы из легитимного UI работают в штатном режиме.
- GREEN: `docs/fix_logs/bug-91-green.log`; строка `=== GATE OK bug N | diff ... ===` скопирована из вывода:
  `=== GATE OK bug 91 | diff f5c35ac1edd0 | лог docs/fix_logs/bug-91-green.log ===`
- Статус: FIXED

### Баг 90 — CSRF через GET /api/servers/speedtest/{id}
- Правила перечитаны: 2026-11-05 01:53:07 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/main.rs:758` — регистрация маршрута `/api/servers/speedtest/{id}`
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
  .route("/api/servers/speedtest", post(api::speedtest_server))
  .route("/api/servers/speedtest/{id}", post(api::speedtest_server_by_id).get(api::speedtest_server_by_id))
  .route("/api/servers/import-node", post(api::import_node))
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: Мутирующее действие (запуск speedtest с переключением текущего активного узла и группы Mihomo) было доступно через HTTP метод GET. При переходе по внешней ссылке или загрузке `<img src="http://192.168.2.1:1001/api/servers/speedtest/..." />` браузер отправляет cookie (SameSite=Lax), что приводило к CSRF-переключению узла и нарушению интернет-связи.
- Первопричина одним предложением: Маршрут `/api/servers/speedtest/{id}` ошибочно принимал GET-запросы наряду с POST.
- Тест: `backend/src/auth.rs::regression_bug_90_get_speedtest_disallowed_in_routes`; spec: `be:regression_bug_90`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): проверяет, что в маршрутах приложения `main.rs` для `/api/servers/speedtest/{id}` отсутствует привязка `.get(..)`.
- RED: `docs/fix_logs/bug-90-red.log`, реальная причина падения: `thread 'auth::tests::regression_bug_90_get_speedtest_disallowed_in_routes' (13352) panicked at src\auth.rs:850:9: GET-обработчик для /api/servers/speedtest/{id} должен быть удален для защиты от CSRF через GET`
- Исправление (что и почему, 1–3 предложения): Удален обработчик `.get(api::speedtest_server_by_id)` из маршрута в `main.rs`. Теперь эндпоинт доступен исключительно через метод POST с защитой CSRF.
- Какие инварианты затрагивает и почему не нарушены: Мутирующие действия только через защищённые методы с проверкой Origin/CSRF-токена.
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Не задеты, фронтенд выполняет замер скорости исключительно через `POST /api/servers/speedtest`.
- GREEN: `docs/fix_logs/bug-90-green.log`; строка `=== GATE OK bug N | diff ... ===` скопирована из вывода:
  `=== GATE OK bug 90 | diff 66e4cad53b5e | лог docs/fix_logs/bug-90-green.log ===`
- Статус: FIXED

### Баг 141 — Утечка токена Telegram в GET /api/settings и затирание секретов плейсхолдером ******
- Правила перечитаны: 2026-11-05 02:00:03 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/api.rs:577` — `get_settings` / `mask_settings_for_api`, `backend/src/api.rs:658` — `merge_settings_update`
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
  if let Some(adguard) = val.get_mut("adguard") {
      if let Some(pw) = adguard.get_mut("password") {
          if !pw.as_str().unwrap_or("").is_empty() {
              *pw = serde_json::Value::String("******".into());
          }
      }
  }
  api_ok(val)
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: Токен Telegram-бота отдавался открытым текстом в `GET /api/settings` (маскировались только пароли auth, rci, mihomo, adguard). Кроме того, `merge_settings_update` не обрабатывал значение `"******"` для секретов — если клиент присылал полученные настройки обратно, реальные секреты перезаписывались строкой `"******"`.
- Первопричина одним предложением: В `get_settings` отсутствовала маскировка `notifications.telegram_bot_token`, а в `merge_settings_update` отсутствовало восстановление исходных секретов при получении плейсхолдера `"******"`.
- Тест: `backend/src/api.rs::regression_bug_141_mask_telegram_token_and_preserve_placeholders`; spec: `be:regression_bug_141`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): вызывает `mask_settings_for_api` и проверяет маскировку токена `"******"`, затем вызывает `merge_settings_update` с плейсхолдерами и проверяет сохранность оригинальных секретов.
- RED: `docs/fix_logs/bug-141-red.log`, реальная причина падения: `assertion left == right failed: Токен Telegram-бота должен маскироваться '******' при отдаче в UI. left: Some("123456789:ABCdefGHIjklMNOpqrs"), right: Some("******")`
- Исправление (что и почему, 1–3 предложения): В `mask_settings_for_api` добавлено маскирование `notifications.telegram_bot_token`. В `merge_settings_update` добавлено восстановление исходных секретов (токен Telegram, пароли RCI, секрет Mihomo, пароль AdGuard), если передан плейсхолдер `"******"`.
- Какие инварианты затрагивает и почему не нарушены: Секреты не уходят в логи, Telegram, URL, открытые GET-ответы.
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Не задеты.
- GREEN: `docs/fix_logs/bug-141-green.log`; строка `=== GATE OK bug N | diff ... ===` скопирована из вывода:
  `=== GATE OK bug 141 | diff 38d4d73e0c66 | лог docs/fix_logs/bug-141-green.log ===`
- Статус: FIXED

### Баг 61 — Бэкап через Telegram-бота отправляет секреты в открытом виде
- Правила перечитаны: 2026-11-05 02:04:28 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/bot.rs:1290` — `sanitize_config_for_backup`, `backend/src/bot.rs:1348` — `handle_backup_request`
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
  match tokio::fs::read(&*state.config_path).await {
      Ok(bytes) => {
          let filename = format!("xkeen-route-config-{}.json", chrono::Local::now().format("%Y%m%d-%H%M"));
          let caption = format!(...);
          if let Err(e) = send_doc(http, token, chat_id, &filename, bytes, &caption).await {
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: Команда `/backup` или кнопка "Бэкап config" в Telegram-боте читала `config.json` и отправляла его в чат Telegram как документ в исходном незашифрованном виде со всеми секретами (хеш пароля, соль, session_secret, токен бота, пароли RCI, секрет Mihomo, пароль AdGuard, секретные URL/токены подписок).
- Первопричина одним предложением: В `handle_backup_request` отсутствовала санитизация конфигурации перед отправкой документа в Telegram.
- Тест: `backend/src/bot.rs::regression_bug_61_sanitize_config_backup_for_telegram`; spec: `be:regression_bug_61`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): вызывает `sanitize_config_for_backup` с полным набором секретов в конфиге и проверяет, что в полученном для отправки файле нет ни одного реального секрета/токена.
- RED: `docs/fix_logs/bug-61-red.log`, реальная причина падения: `thread 'bot::tests::regression_bug_61_sanitize_config_backup_for_telegram' (10524) panicked at src\bot.rs:1712:9: хеш пароля не должен присутствовать в бэкапе`
- Исправление (что и почему, 1–3 предложения): Реализована функция `sanitize_config_for_backup`, которая маскирует поля `auth` (хеш, соль, session_secret), `notifications` (токен бота, webhook url), `rci` (пароль, токен), `mihomo` (секрет), `adguard` (пароль) и `mobile_provider_urls` строкой `"******"`. В `handle_backup_request` отправляются только санитизированные байты.
- Какие инварианты затрагивает и почему не нарушены: Секреты не уходят в логи, Telegram, URL, открытые ответы.
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Не задеты.
- GREEN: `docs/fix_logs/bug-61-green.log`; строка `=== GATE OK bug N | diff ... ===` скопирована из вывода:
  `=== GATE OK bug 61 | diff a01511fa2fc5 | лог docs/fix_logs/bug-61-green.log ===`
- Статус: FIXED

### Баг 128 — Telegram-бот авторизует по chat.id, а не from.id
- Правила перечитаны: 2026-11-05 02:09:05 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/bot.rs:236` — `handle_update`, `backend/src/bot.rs:278` — `is_authorized_sender`
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
  if let Some(cb) = update.callback_query {
      let chat_id_str = cb
          .message
          .as_ref()
          .map(|m| m.chat.id.to_string())
          .unwrap_or_default();

      if !is_authorized(&chat_id_str, target_chat_id) {
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: Если в качестве target_chat_id настроена группа для уведомлений (отрицательный ID) или сообщение/callback приходит из группы, проверка `chat_id == target_chat_id` позволяла любому участнику группы управлять роутером (переключение серверов, кнопка паники, обновления, бэкап с секретами).
- Первопричина одним предложением: В `handle_update` проверка доступа выполнялась по `chat.id` вместо проверки личного ID пользователя-отправителя `from.id`.
- Тест: `backend/src/bot.rs::regression_bug_128_bot_authorizes_by_from_id_not_chat_id`; spec: `be:regression_bug_128`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): вызывает `is_authorized_sender` с чужим `from_id` при совпадающем групповом `chat_id` и проверяет, что доступ запрещается (`false`).
- RED: `docs/fix_logs/bug-128-red.log`, реальная причина падения: `thread 'bot::tests::regression_bug_128_bot_authorizes_by_from_id_not_chat_id' (4196) panicked at src\bot.rs:1799:9: Произвольный участник группы не должен авторизоваться только из-за совпадения chat.id с ID группы`
- Исправление (что и почему, 1–3 предложения): Добавлены поля `from` в структуры `TelegramMessage` и `TelegramCallbackQuery`. Реализована функция `is_authorized_sender`, проверяющая именно `from.id` на совпадение с доверенным ID владельца. В группах произвольным участникам доступ к управляющим командам роутера запрещён.
- Какие инварианты затрагивает и почему не нарушены: Безопасность и контроль доступа к роутеру; легитимный владелец в личном чате сохраняет полный контроль.
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Не задеты.
- GREEN: `docs/fix_logs/bug-128-green.log`; строка `=== GATE OK bug N | diff ... ===` скопирована из вывода:
  `=== GATE OK bug 128 | diff debdeee8be12 | лог docs/fix_logs/bug-128-green.log ===`
- Статус: FIXED

### Баг 44 — AdGuard открыт в WAN через apcli+ и неограниченный INPUT ACCEPT
- Правила перечитаны: 2026-11-05 02:14:33 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/adguard.rs:1162` — `AGH_LAN_INTERFACES`, `backend/src/adguard.rs:1165` — `build_adguard_ndm_hook_content`, `backend/src/adguard.rs:1200` — `apply_adguard_iptables`
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
  pub const AGH_LAN_INTERFACES: &[&str] = &["br+", "Bridge+", "ra+", "apcli+"];
  ...
  iptables -t filter -C INPUT -p udp --dport {port} -j ACCEPT 2>/dev/null || iptables -t filter -I INPUT 1 -p udp --dport {port} -j ACCEPT 2>/dev/null || true
  iptables -t filter -C INPUT -p tcp --dport {port} -j ACCEPT 2>/dev/null || iptables -t filter -I INPUT 1 -p tcp --dport {port} -j ACCEPT 2>/dev/null || true
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: Интерфейс `apcli+` является интерфейсом беспроводного клиента WISP (WAN) на Keenetic. Включение `apcli+` приводило к перехвату DNS со стороны сети провайдера. Правило `INPUT` в таблице filter не содержало ограничения по входящему интерфейсу (`-i`), в результате порт AdGuard Home был открыт для любых входящих запросов из интернета/WAN.
- Первопричина одним предложением: В `AGH_LAN_INTERFACES` был ошибочно включен WAN-интерфейс `apcli+`, а правила `INPUT ACCEPT` не ограничивались LAN-интерфейсами.
- Тест: `backend/src/adguard.rs::regression_bug_44_adguard_not_exposed_to_wan`; spec: `be:regression_bug_44`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): проверяет отсутствие `apcli+` в `AGH_LAN_INTERFACES` и вызов `build_adguard_ndm_hook_content` с проверкой привязки правил `INPUT` к LAN/lo интерфейсам.
- RED: `docs/fix_logs/bug-44-red.log`, реальная причина падения: `thread 'adguard::tests::regression_bug_44_adguard_not_exposed_to_wan' (11136) panicked at src\adguard.rs:2917:9: apcli+ является WAN/WISP интерфейсом и не должен присутствовать в AGH_LAN_INTERFACES`
- Исправление (что и почему, 1–3 предложения): Интерфейс `apcli+` удален из `AGH_LAN_INTERFACES` и NDM hook'а. Правила `INPUT ACCEPT` в `apply_adguard_iptables` и в NDM hook'е ограничены исключительно LAN-интерфейсами (`br+`, `Bridge+`, `ra+`) и loopback (`lo`). В `remove_adguard_iptables` добавлена очистка интерфейсных правил `INPUT`.
- Какие инварианты затрагивает и почему не нарушены: iptables: безопасные правила без открытия портов в WAN, парная очистка, идемпотентность.
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Не задеты; LAN клиенты продолжают штатно фильтроваться через AdGuard.
- GREEN: `docs/fix_logs/bug-44-green.log`; строка `=== GATE OK bug N | diff ... ===` скопирована из вывода:
  `=== GATE OK bug 44 | diff 37d66c313e84 | лог docs/fix_logs/bug-44-green.log ===`
- Статус: FIXED

### Баг 89 — SSRF-фильтр вебхука проверяет только литералы IP и уязвим к DNS и редиректам
- Правила перечитаны: 2026-11-05 02:20:24 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/notifications.rs:185` — `validate_webhook_url`, `backend/src/notifications.rs:274` — `send_webhook`
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
  let host_unbracketed = host.trim_start_matches('[').trim_end_matches(']');
  if let Ok(ip) = host_unbracketed.parse::<std::net::IpAddr>() {
      let is_unsafe = match ip { ... };
      if is_unsafe {
          return Err("Запрещен вызов внутренних/локальных (private, loopback, link-local) адресов через Webhook".to_string());
      }
  }
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: Проверка адреса вебхука выполнялась только для литералов IP, пропуская доменные имена с локальными суффиксами (.lan, .local, .internal, .home.arpa), доменные имена, разрешающиеся в приватные IP роутера (SSRF / DNS rebinding), а клиент reqwest следовал HTTP-редиректам (позволяя внешнему серверу перенаправить запрос на локальный интерфейс роутера).
- Первопричина одним предложением: Валидация проверяла только явные IP-литералы без проверки доменных суффиксов и без резолвинга DNS, а HTTP-клиент вебхука следовал редиректам.
- Тест: `backend/src/notifications.rs::regression_bug_89_ssrf_dns_and_redirects_prevention`; spec: `be:regression_bug_89`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): вызывает `validate_webhook_url` для локальных доменных зон и `is_unsafe_ip` для классификации небезопасных IP адресов, ожидая блокировку.
- RED: `docs/fix_logs/bug-89-red.log`, реальная причина падения: `thread 'notifications::tests::regression_bug_89_ssrf_dns_and_redirects_prevention' (6384) panicked at src\notifications.rs:348:9: Домены .lan должны блокироваться SSRF-фильтром`
- Исправление (что и почему, 1–3 предложения): В `validate_webhook_url` добавлена блокировка доменов с локальными зонами (.lan, .local, .internal, .home.arpa, .localhost). Реализована функция `resolve_and_validate_webhook_host` для разрешения DNS и проверки результирующих IP адресов. В `send_webhook` клиент переведен на `redirect::Policy::none()`.
- Какие инварианты затрагивает и почему не нарушены: Защита роутера и локальной сети от SSRF.
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Не задеты; легитимные публичные вебхуки (Discord, Slack, Telegram) отправляются штатно.
- GREEN: `docs/fix_logs/bug-89-green.log`; строка `=== GATE OK bug N | diff ... ===` скопирована из вывода:
  `=== GATE OK bug 89 | diff 36961afe2296 | лог docs/fix_logs/bug-89-green.log ===`
- Статус: FIXED

### Баг 92 — Синхронный PBKDF2 в потоке tokio и обход rate limit при параллельных попытках
- Правила перечитаны: 2026-11-05 02:24:52 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/auth.rs:117` — `check_and_reserve_rate_limit`, `backend/src/auth.rs:160` — `verify_password_async`, `backend/src/auth.rs:395` — `login`, `backend/src/auth.rs:493` — `change_password`
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
  if !check_rate_limit(client_ip) {
      return (StatusCode::TOO_MANY_REQUESTS, ...);
  }
  ...
  if !verify_password(&body.password, &auth_cfg.salt, &auth_cfg.password_hash) {
      record_failed_attempt(client_ip);
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: PBKDF2 (100k итераций) вычислялся синхронно прямо в контексте потока tokio, блокируя весь рантайм (watchdog, failover) на 0.5–1 с. При параллельных запросах проверка `check_rate_limit` выполнялась до записи попытки в `record_failed_attempt`, что позволяло параллельным запросам обходить лимит 5/мин.
- Первопричина одним предложением: PBKDF2 не выносился в пул блокирующих задач `spawn_blocking`, а лимит попыток проверялся без атомарного инкремента/резервирования слота до вычисления хеша.
- Тест: `backend/src/auth.rs::regression_bug_92_rate_limit_and_spawn_blocking`; spec: `be:regression_bug_92`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): вызывает `check_and_reserve_rate_limit` и проверяет блокировку после 5 попыток без необходимости ручного вызова `record_failed_attempt`, а также проверяет вызов `verify_password_async` / `spawn_blocking` в auth.rs.
- RED: `docs/fix_logs/bug-92-red.log`, реальная причина падения: `thread 'auth::tests::regression_bug_92_rate_limit_and_spawn_blocking' (9064) panicked at src\auth.rs:920:9: 6-я попытка должна быть заблокирована rate-limit`
- Исправление (что и почему, 1–3 предложения): Реализована функция `check_and_reserve_rate_limit`, атомарно проверяющая лимит и регистрирующая попытку сразу до запуска вычислений. Реализованы функции `verify_password_async` и `hash_password_async`, выполняющие ресурсоемкие операции PBKDF2 в `tokio::task::spawn_blocking`. В `login` и `change_password` применены асинхронные методы.
- Какие инварианты затрагивает и почему не нарушены: Защита рантайма tokio от зависания и DoS-атак на роутере.
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Не задеты; watchdog и failover теперь не блокируются при входе в панель.
- GREEN: `docs/fix_logs/bug-92-green.log`; строка `=== GATE OK bug N | diff ... ===` скопирована из вывода:
  `=== GATE OK bug 92 | diff a529d479f29c | лог docs/fix_logs/bug-92-green.log ===`
- Статус: FIXED

### Баг 93 — Отозванные токены хранятся только в памяти и восстанавливаются после перезапуска
- Правила перечитаны: 2026-11-05 02:28:54 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/auth.rs:74` — `init_revoked_tokens_storage`, `backend/src/auth.rs:107` — `persist_revoked_tokens`, `backend/src/auth.rs:124` — `revoke_token`, `backend/src/main.rs:451` — инициализация в сервере
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
  static REVOKED_TOKENS: std::sync::Mutex<Option<std::collections::HashMap<String, u64>>> = std::sync::Mutex::new(None);

  pub fn revoke_token(token: &str) {
      ...
      let exp = now + SESSION_TTL_SECS;
      map.insert(token.to_string(), exp);
  }
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: Отозванные токены (logout) сохранялись только в памяти процесса. При перезапуске сервиса или роутера список отозванных токенов очищался, в результате чего старые токены сессий снова становились действительными вплоть до истечения TTL (до 24 часов).
- Первопричина одним предложением: Список отозванных токенов не сохранялся на постоянном диске между перезапусками приложения.
- Тест: `backend/src/auth.rs::regression_bug_93_revoked_tokens_persist_across_restart`; spec: `be:regression_bug_93`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): вызывает `revoke_token`, сбрасывает кэш в памяти `reset_in_memory_revoked_tokens_for_test()`, повторно инициализирует хранилище из файла и проверяет, что `is_token_revoked` возвращает `true`.
- RED: `docs/fix_logs/bug-93-red.log`, реальная причина падения: `thread 'auth::tests::regression_bug_93_revoked_tokens_persist_across_restart' (11284) panicked at src\auth.rs:991:9: После перезапуска отозванный токен должен оставаться отозванным (прочитан с диска)`
- Исправление (что и почему, 1–3 предложения): Реализованы функции `init_revoked_tokens_storage` и `persist_revoked_tokens`, сохраняющие отозванные токены в файл `revoked_tokens.json` рядом с `config.json` с атомарной перезаписью. При старте сервиса неистёкшие отозванные токены восстанавливаются в память.
- Какие инварианты затрагивает и почему не нарушены: Постоянство состояния безопасности авторизации между перезапусками сервиса.
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Не задеты.
- GREEN: `docs/fix_logs/bug-93-green.log`; строка `=== GATE OK bug N | diff ... ===` скопирована из вывода:
  `=== GATE OK bug 93 | diff 74475dad7261 | лог docs/fix_logs/bug-93-green.log ===`
- Статус: FIXED

### Баг 68 — Проверка SHA-256 обновления пропускается при отсутствии опубликованного хеша
- Правила перечитаны: 2026-11-05 02:50:54 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/updater.rs:72` — `verify_release_checksum`, `backend/src/updater.rs:615` — `install_update`
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
  if let Some(exp) = expected_sha {
      if !verify_sha256(&calculated_sha256, &exp) {
          return api_err(format!("Ошибка верификации SHA-256..."));
      }
  } else {
      crate::log_w!("[UPDATE] ⚠️ Контрольная сумма не опубликована в релизе ... верификация SHA-256 пропущена.");
  }
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: Проверка SHA-256 была заявлена как обязательная преграда перед заменой бинарника от root, однако если в описании релиза и файле `{asset}.sha256` хеш отсутствовал, установка продолжалась с предупреждением в лог, что открывало возможность подмены бинарника.
- Первопричина одним предложением: В `install_update` ветка отсутствия ожидаемого хеша (`expected_sha.is_none()`) не прерывала процесс установки, пропуская непроверенный бинарник.
- Тест: `backend/src/updater.rs::regression_bug_68_mandatory_sha256_verification`; spec: `be:regression_bug_68`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): вызывает `verify_release_checksum` с `expected_sha256: None` и проверяет, что возвращается ошибка (`Err`).
- RED: `docs/fix_logs/bug-68-red.log`, реальная причина падения: `thread 'updater::tests::regression_bug_68_mandatory_sha256_verification' (9096) panicked at src\updater.rs:1563:9: Отсутствие опубликованной контрольной суммы должно прерывать установку бинарника`
- Исправление (что и почему, 1–3 предложения): Реализована функция `verify_release_checksum`, которая требует обязательного наличия опубликованного хеша и отклоняет установку с `Err`, если контрольная сумма отсутствует или не совпадает. В `install_update` при ошибке верификации скачанный временный файл удаляется и возвращается `api_err`.
- Какие инварианты затрагивает и почему не нарушены: Безопасность обновлений: исполнение бинарников только при подтверждённой целостности.
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Не задеты.
- GREEN: `docs/fix_logs/bug-68-green.log`; строка `=== GATE OK bug N | diff ... ===` скопирована из вывода:
  `=== GATE OK bug 68 | diff 1ae18a4707cb | лог docs/fix_logs/bug-68-green.log ===`
- Статус: FIXED

### Баг 69 — Хеш берётся из того же релиза, что и бинарник; отсутствует цифровая подпись релизов
- Правила перечитаны: 2026-11-05 02:52:31 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/updater.rs:580` — `install_update`
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
  let mut expected_sha = extract_hash_from_checksum_text(&rel.body, chosen_asset);
  if expected_sha.is_none() {
      let sums_url = format!("{GITHUB_RELEASE}/{ver}/{chosen_asset}.sha256");
      match http.get(&sums_url)...
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: Хеш загружается из того же GitHub Release, что и бинарник (из описания релиза или соседнего ассета `{asset}.sha256`). В случае компрометации репозитория/токена релиза злоумышленник может одновременно заменить и бинарник, и sha256. В проекте отсутствует криптографическая подпись артефактов (ed25519 / minisign).
- Первопричина одним предложением: Отсутствует механизм валидации асимметричной цифровой подписи релиза доверенным публичным ключом разработчика.
- Тест: -
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): -
- RED: -
- Исправление (что и почему, 1–3 предложения): Требуется продуктовое и архитектурное решение о схеме цифровой подписи релизов.
- Какие инварианты затрагивает и почему не нарушены: Безопасность цепочки поставок обновлений.
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Не задеты.
- GREEN: -
- Статус: BLOCKED (Вопрос пользователю: Вариант 1: Внедрить Minisign/ed25519 цифровую подпись артефактов (файл .minisig) с зашитым публичным ключом в бинарник и генерацией подписи в CI/CD GitHub Actions; Вариант 2: Загружать контрольные суммы из независимого доверенного DNS/TXT/HTTPS манифеста; Рекомендую Вариант 1 как стандарт для OpenWrt/Entware).

### Баг 1 — При падении AdGuard в режиме БС вся сеть теряет DNS
- Правила перечитаны: 2026-11-05 02:59:42 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/adguard.rs:1165` — `build_sync_tunnel_dns_ports_script`, `backend/src/adguard.rs:1180` — `build_adguard_ndm_hook_content`, `backend/src/adguard.rs:1268` — `apply_adguard_iptables`, `backend/src/adguard.rs:1350` — `remove_adguard_iptables`, `backend/src/api.rs:10370` — `resolve_active_dns_port_with_health`
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
  iptables -t nat -S XKEEN_GAMING 2>/dev/null | grep -E 'to-ports (1053|5353|5354)' | while read -r line; do \
      del_cmd=$(echo "$line" | sed 's/^-A/-D/'); \
      add_cmd=$(echo "$line" | sed -E 's/--to-ports [0-9]+/--to-ports {port}/'); \
      iptables -t nat $del_cmd 2>/dev/null || true; \
      iptables -t nat $add_cmd 2>/dev/null || true; \
  done
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: При остановке или падении AdGuard переключение DNS-портов в `remove_adguard_iptables` и хуке затрагивало только цепочку `XKEEN_GAMING`. Цепочка `XKEEN_BS`, стоящая первой в PREROUTING, продолжала перенаправлять все DNS-запросы на мёртвый порт AdGuard (5353/5354), приводя к потере интернета во всей сети. Кроме того, `resolve_active_dns_port` не учитывал состояние здоровья AdGuard и возвращал мёртвый порт.
- Первопричина одним предложением: В правилах синхронизации фаервола и NDM-хуке переключение портов выполнялось только для `XKEEN_GAMING` без учёта `XKEEN_BS`, а `resolve_active_dns_port` не учитывал падение службы.
- Тест: `backend/src/api.rs::regression_bug_1_adguard_failover_bs_mode_dns`; spec: `be:regression_bug_1_adguard`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): вызывает `build_adguard_ndm_hook_content` и `build_sync_tunnel_dns_ports_script`, проверяя наличие `XKEEN_BS`, а также `resolve_active_dns_port_with_health(&cfg, false)` с проверкой отката на 1053 при падении AdGuard.
- RED: `docs/fix_logs/bug-1-red.log`, реальная причина падения: `thread 'api::settings_and_service_regression_tests::regression_bug_1_adguard_failover_bs_mode_dns' (6756) panicked at src\api.rs:10955:9: NDM-хук AdGuard обязан переключать DNS и в цепочке XKEEN_BS при падении/восстановлении AdGuard`
- Исправление (что и почему, 1–3 предложения): Создана функция `build_sync_tunnel_dns_ports_script`, синхронизирующая порты для обеих цепочек: `XKEEN_GAMING` и `XKEEN_BS`. `remove_adguard_iptables` и NDM-хук теперь восстанавливают порт 1053 в обеих цепочках. В `resolve_active_dns_port_with_health` добавлен откат на 1053 при недоступности AdGuard.
- Какие инварианты затрагивает и почему не нарушены: AdGuard вкл ⇔ редирект 53→порт AGH; защита от потери DNS всей сетью при отказе службы.
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Не задеты, обе цепочки синхронизируются согласованно.
- GREEN: `docs/fix_logs/bug-1-green.log`; строка `=== GATE OK bug N | diff ... ===` скопирована из вывода:
  `=== GATE OK bug 1 | diff 1b692f098ee5 | лог docs/fix_logs/bug-1-green.log ===`
- Статус: FIXED

### Баг 2 — Исключение устройства из БС не работает для обычных устройств
- Правила перечитаны: 2026-11-05 03:08:37 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/routing.rs:2054` — `find_ip_for_mac_in_arp` / `resolve_excluded_whitelist_ips_internal`, `backend/src/api.rs:11930` — `toggle_whitelist_exclude`
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
  if let Some(dev) = cfg.gaming.devices.iter().find(|d| d.mac.eq_ignore_ascii_case(clean)) {
      if !dev.ip.trim().is_empty() { ips.push(dev.ip.trim().to_string()); }
  }
  if let Some(dev) = cfg.whitelist.devices.iter().find(|d| d.mac.eq_ignore_ascii_case(clean)) {
      if !dev.ip.trim().is_empty() { ips.push(dev.ip.trim().to_string()); }
  }
  if let Some((ip, _)) = cfg.device_routing.iter().find(|(k, _)| k.eq_ignore_ascii_case(clean)) {
      ips.push(ip.clone());
  }
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: UI отправляет MAC устройства сети. Функция `resolve_excluded_whitelist_ips` искала соответствие MAC -> IP только в явных списках `gaming.devices` и `whitelist.devices`. Обычные устройства домашней сети роутера не зарегистрированы в этих списках, а поиск по `device_routing` не срабатывал, так как ключами там являются IP-адреса. В итоге исключение для обычных устройств не попадало в iptables и Mihomo rules, и они продолжали проксироваться через БС.
- Первопричина одним предложением: В `resolve_excluded_whitelist_ips` отсутствовал поиск IP по MAC в системной ARP-таблице роутера для обычных устройств сети.
- Тест: `backend/src/routing.rs::regression_bug_2_resolve_excluded_whitelist_ips_for_regular_devices`; spec: `be:regression_bug_2_resolve_excluded`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): вызывает `find_ip_for_mac_in_arp` и `resolve_excluded_whitelist_ips_internal` с MAC обычного устройства и содержимым ARP-таблицы, ожидая корректное извлечение IP.
- RED: `docs/fix_logs/bug-2-red.log`, реальная причина падения: `thread 'routing::tests::regression_bug_2_resolve_excluded_whitelist_ips_for_regular_devices' (13272) panicked at src\\routing.rs:4862:9: resolve_excluded_whitelist_ips обязан находить IP обычного устройства по его MAC из ARP`
- Исправление (что и почему, 1–3 предложения): Реализован парсер системной таблицы ARP `find_ip_for_mac_in_arp` и интеграция с чтением `/proc/net/arp` в `resolve_excluded_whitelist_ips_internal`. В `toggle_whitelist_exclude` добавлена поддержка сохранения IP вместе с MAC.
- Какие инварианты затрагивает и почему не нарушены: Zero-Touch: корректное исключение любого устройства сети из перехвата БС без ручных правок.
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Не задеты; исключенные устройства отправляются в DIRECT, остальные защищены БС.
- GREEN: `docs/fix_logs/bug-2-green.log`; строка `=== GATE OK bug N | diff ... ===` скопирована из вывода:
  `=== GATE OK bug 2 | diff 0eb4fb43cbc5 | лог docs/fix_logs/bug-2-green.log ===`
- Статус: FIXED

### Баг 3 — В режиме БС (StrictXKeen) правила, стоящие выше catch-all блока БС, обходят его
- Правила перечитаны: 2026-11-05 03:14:54 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/routing.rs:2315` — `apply_whitelist_rules`, `backend/src/routing.rs:3240` — `apply_routing_with_filters`
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
  let target_insert_idx = if let Some(dev_rules_idx) = lines[rules_pos + 1..].iter().position(|l| l.trim() == RULES_BEGIN) {
      rules_pos + 1 + dev_rules_idx
  } else if let Some(fallback_idx) = lines[rules_pos + 1..].iter().position(|l| {
      let t = l.trim();
      t.starts_with("- GEOIP,") || t.starts_with("- MATCH,")
  }) {
      rules_pos + 1 + fallback_idx
  } else {
      rules_pos + 1
  };
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: В режиме белых списков StrictXKeen правила доменов (force_domains), zapret-hybrid и gaming вставлялись выше блока AUTO-WHITELIST. В результате запросы к этим доменам направлялись в группу PROXY (обычные VPS), создавая запрещённый третий канал трафика мимо туннеля белых списков и нарушая работу белых списков.
- Первопричина одним предложением: В `apply_routing` и `apply_whitelist_rules` вставка правил БС производилась над правилами устройств, но под правилами доменов, zapret и gaming.
- Тест: `backend/src/routing.rs::regression_bug_3_whitelist_strict_mode_rule_hierarchy`; spec: `be:regression_bug_3_whitelist_strict`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): вызывает `apply_routing` с активным `StrictXKeen` и `force_domains`, проверяя, что позиция `WHITELIST_BEGIN` строго предшествует `FORCE_BEGIN` в секции `rules:`.
- RED: `docs/fix_logs/bug-3-red.log`, реальная причина падения: `thread 'routing::tests::regression_bug_3_whitelist_strict_mode_rule_hierarchy' (4544) panicked at src\routing.rs:4907:9: В режиме StrictXKeen правила белых списков (pos 4986) обязаны быть ВЫШЕ force_domains (pos 4895), чтобы исключить утечку в третий канал PROXY`
- Исправление (что и почему, 1–3 предложения): В `apply_routing` изменён порядок наложения блоков, а в `apply_whitelist_rules` при стратегии `StrictXKeen` целевой индекс вставки в `rules:` вычисляется на самом верху (сразу под `AdBlock`, если он есть, или сразу после `rules:`). Это гарантирует, что отечественные сервисы идут в `DIRECT`, а весь внешний трафик направляется в `🛡️ Whitelist` без утечки в группу `PROXY`.
- Какие инварианты затрагивает и почему не нарушены: Строго два канала трафика: Direct + (БС ИЛИ VPS), никакого третьего канала.
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Не задеты; при выключенном БС или режиме Standard иерархия сохраняется штатной.
- GREEN: `docs/fix_logs/bug-3-green.log`; строка `=== GATE OK bug N | diff ... ===` скопирована из вывода:
  `=== GATE OK bug 3 | diff a42402815e19 | лог docs/fix_logs/bug-3-green.log ===`
- Статус: FIXED

### Баг 70 — Ни один iptables-вызов из Rust не использует `-w`
- Правила перечитаны: 2026-11-05 03:31:13 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/api.rs:10385` (`build_gaming_full_tunnel_commands`), `backend/src/api.rs:10650` (`build_whitelist_full_tunnel_commands`), `backend/src/adguard.rs:1165` (`build_sync_tunnel_dns_ports_script`), `backend/src/adguard.rs:1180` (`build_adguard_ndm_hook_content`), `backend/src/direct_traffic.rs:10` (`init_direct_iptables`, `cleanup_direct_iptables`, `read_direct_bytes`)
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
  while iptables -t nat -D PREROUTING -j XKEEN_GAMING 2>/dev/null; do :; done; \
  iptables -t nat -F XKEEN_GAMING 2>/dev/null || true; \
  iptables -t nat -X XKEEN_GAMING 2>/dev/null || true; \
  while iptables -t mangle -D PREROUTING -j XKEEN_GAMING 2>/dev/null; do :; done; \
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: Команды iptables и ip6tables вызывались без флага ожидания блокировки `-w` (wait lock). Каждую секунду `direct_traffic::read_direct_bytes` вызывал `iptables -t filter -L`, что приводило к захвату xtables lock и ошибке exit code 4 в параллельных вызовах AdGuard, БС и гейминга. Кроме того, при любой ошибке чтения вызывался `init_direct_iptables` со сбросом счётчиков (`-F`).
- Первопричина одним предложением: Отсутствие параметров ожидания блокировки `-w 2` в вызовах и генераторах команд `iptables`/`ip6tables`, а также сброс счётчиков при временных сбоях чтения.
- Тест: `backend/src/api.rs::regression_bug_70_all_iptables_commands_use_wait_lock`; spec: `be:regression_bug_70`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): вызывает сборщики команд `build_gaming_full_tunnel_commands`, `build_whitelist_full_tunnel_commands`, `build_adguard_ndm_hook_content`, `build_sync_tunnel_dns_ports_script` и проверяет, что каждая сгенерированная команда с `iptables ` содержит `-w`.
- RED: `docs/fix_logs/bug-70-red.log`, реальная причина падения: `thread 'api::settings_and_service_regression_tests::regression_bug_70_all_iptables_commands_use_wait_lock' (8328) panicked at src\api.rs:10996:17: Команда очистки Gaming должна содержать -w: 'while iptables -t nat -D PREROUTING -j XKEEN_GAMING 2>/dev/null'`
- Исправление (что и почему, 1–3 предложения): Добавлен флаг `-w 2` во все вызовы `Command::new("iptables")` и `Command::new("ip6tables")` в `direct_traffic.rs`, `adguard.rs`, а также в сгенерированные скрипты в `api.rs` с сохранением полной обратной совместимости. В `direct_traffic.rs` сброс правил через `init_direct_iptables` теперь выполняется только в том случае, если цепочка действительно отсутствует в ядре.
- Какие инварианты затрагивает и почему не нарушены: iptables: `-w`, идемпотентно, есть парная очистка. Никаких race condition при параллельной работе сервисов роутера.
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Не задеты, все режимы защищены от блокировки xtables lock.
- GREEN: `docs/fix_logs/bug-70-green.log`; строка `=== GATE OK bug N | diff ... ===` скопирована из вывода:
  `=== GATE OK bug 70 | diff a60e19b65082 | лог docs/fix_logs/bug-70-green.log ===`
- Статус: FIXED

### Баг 71 — Цепочка XKEEN_DIRECT: потеря перехода из FORWARD не детектится
- Правила перечитаны: 2026-11-05 03:34:19 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/direct_traffic.rs:116` (`read_direct_bytes`), `backend/src/direct_traffic.rs:153` (`is_direct_iptables_active`)
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
  let out = std::process::Command::new("iptables")
      .args(["-w", "2", "-t", "filter", "-L", DIRECT_CHAIN, "-v", "-n", "-x"])
      .output()
      .ok()?;

  if out.status.success() {
      let s = String::from_utf8_lossy(&out.stdout);
      return parse_iptables_direct_bytes(&s);
  }
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: Если KeeneticOS ndm перестраивал цепочку FORWARD, переход `-j XKEEN_DIRECT` исчезал из FORWARD. При этом сама цепочка XKEEN_DIRECT оставалась в ядре, из-за чего `read_direct_bytes` успешно читал старые замершие значения байт, считал что учёт работает, и оставлял флаг аппаратного учёта `DIRECT_IPTABLES_ACTIVE = true`.
- Первопричина одним предложением: В `read_direct_bytes` проверялся только успешный статус чтения цепочки `XKEEN_DIRECT` и не проверялось фактическое наличие правила jump в родительской цепочке `FORWARD`.
- Тест: `backend/src/direct_traffic.rs::regression_bug_71_detect_missing_forward_jump`; spec: `be:regression_bug_71`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): вызывает `is_forward_jump_present` с выводом правил FORWARD, проверяя истинность при наличии правила jump и фиксацию потери при отсутствии перенаправления в `XKEEN_DIRECT`.
- RED: `docs/fix_logs/bug-71-red.log`, реальная причина падения: `thread 'direct_traffic::tests::regression_bug_71_detect_missing_forward_jump' (15068) panicked at src\direct_traffic.rs:182:9: Детектор должен находить переход в XKEEN_DIRECT в правилах FORWARD`
- Исправление (что и почему, 1–3 предложения): Реализованы функции `is_forward_jump_present` и `ensure_forward_jump_active()`. При чтении байт в `read_direct_bytes` выполняется проверка наличия перехода в FORWARD; если переход утерян, он автоматически восстанавливается на лету (`iptables -w 2 -t filter -I FORWARD 1 -j XKEEN_DIRECT`) без сброса накопленных счетчиков (`-F`), а при невозможности восстановления флаг `DIRECT_IPTABLES_ACTIVE` сбрасывается в `false`.
- Какие инварианты затрагивает и почему не нарушены: Аппаратный учёт прямого трафика отражает реальное состояние Netfilter, счётчики не сбрасываются.
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Не задеты, правило вставляется первым в FORWARD.
- GREEN: `docs/fix_logs/bug-71-green.log`; строка `=== GATE OK bug N | diff ... ===` скопирована из вывода:
  `=== GATE OK bug 71 | diff 0e5157ff2a2f | лог docs/fix_logs/bug-71-green.log ===`
- Статус: FIXED

### Баг 72 — Счётчик «VPS»/«БС» включает DIRECT-трафик Mihomo
- Правила перечитаны: 2026-11-05 03:38:34 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/traffic.rs:348` (`run_mihomo_stream`)
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
  if is_wl_now {
      WHITELIST_DOWN.store(msg.up, Ordering::Relaxed);
      WHITELIST_UP.store(msg.down, Ordering::Relaxed);
      PROXY_DOWN.store(0, Ordering::Relaxed);
      PROXY_UP.store(0, Ordering::Relaxed);
      TOTAL_WHITELIST_BYTES.fetch_add(bytes, Ordering::Relaxed);
  } else {
      PROXY_DOWN.store(msg.up, Ordering::Relaxed);
      PROXY_UP.store(msg.down, Ordering::Relaxed);
      WHITELIST_DOWN.store(0, Ordering::Relaxed);
      WHITELIST_UP.store(0, Ordering::Relaxed);
      TOTAL_PROXY_BYTES.fetch_add(bytes, Ordering::Relaxed);
  }
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: Поток `/traffic` Mihomo передаёт общий объём байт всех соединений, проходящих через ядро, включая правила `DIRECT` (RU-сайты, Zapret-гибрид, отечественные сервисы). Ранее весь этот объём без разбора списывался в `TOTAL_PROXY_BYTES` или `TOTAL_WHITELIST_BYTES`, что искусственно завышало трафик VPS/БС и занижало счётчик прямого трафика Direct.
- Первопричина одним предложением: В `traffic.rs` отсутствовало разделение соединений Mihomo на DIRECT и PROXY/Whitelist при подсчёте дельт трафика.
- Тест: `backend/src/traffic.rs::regression_bug_72_direct_connections_not_counted_in_vps_traffic`; spec: `be:regression_bug_72`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): вызывает `split_connections_delta` с мокированным ответом `/connections`, проверяя, что соединения с `chains: ["DIRECT"]` или `rule: "DIRECT"` направляются строго в `direct_bytes` и не попадают в `proxy_bytes`.
- RED: `docs/fix_logs/bug-72-red.log`, реальная причина падения: `thread 'traffic::tests::regression_bug_72_direct_connections_not_counted_in_vps_traffic' (6592) panicked at src\traffic.rs:941:9: assertion left == right failed: DIRECT соединения (chains: ['DIRECT']) должны учитываться в direct_bytes. left: 0, right: 10000`
- Исправление (что и почему, 1–3 предложения): Реализованы функции `is_connection_direct` и `split_connections_delta`. DIRECT-соединения классифицируются по цепочкам/правилам и их байты направляются исключительно в счётчик `direct_bytes`, исключая раздувание лимитов `proxy_bytes` и `whitelist_bytes`.
- Какие инварианты затрагивает и почему не нарушены: Точный учёт двух каналов трафика (Direct и Прокси/БС) без взаимного завышения.
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Не задеты, соединения Whitelist и PROXY разделяются корректно.
- GREEN: `docs/fix_logs/bug-72-green.log`; строка `=== GATE OK bug N | diff ... ===` скопирована из вывода:
  `=== GATE OK bug 72 | diff ea94fd8c0dc9 | лог docs/fix_logs/bug-72-green.log ===`
- Статус: FIXED

### Баг 88 — Failover ждёт отправку уведомлений синхронно (до 30 с на мёртвом сервере)
- Правила перечитаны: 2026-11-05 04:25:13 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/notifications.rs:311` — `notify_failover`; 7 вызовов `.await` в `backend/src/failover.rs` (строки 944, 1074, 3112, 3234, 3362, 3385, 3688)
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
          if let Err(e) = send_telegram_with_proxy(http, proxy_url, &cfg.telegram_bot_token, &cfg.telegram_chat_id, &html_msg).await {
              log_w!("Не удалось отправить Telegram уведомление: {}", e);
          }
      ...
      if !cfg.webhook_url.is_empty() {
          if let Err(e) = send_webhook(http, &cfg.webhook_url, event_type, title, details).await {
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: Совпадает с аудитом: send_telegram_with_proxy перебирает прокси Mihomo (timeout 10 с) и прямое соединение (10 с), затем webhook (10 с); failover.rs ждёт `.await` в run_check, в т.ч. уведомление о карантине (3234) до переключения сервера. Подпункты: (а) блокировка run_check, (б) карантинное уведомление до переключения — оба устраняются тем, что вызов больше не ждёт доставки. Проверено локально тестом с «молчащим» прокси; реальную доставку в Telegram не проверял (нет сети/токена).
- Первопричина одним предложением: notify_failover выполнял сетевую доставку прямо в задаче failover, и вызывающий ждал все таймауты.
- Тест: `backend/src/notifications.rs::tests::regression_bug_88_notify_failover_does_not_block_caller`; spec: `be:regression_bug_88`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): Поднимает локальный TCP-«прокси», который принимает соединение и молчит; вызывает рабочий `notify_failover` с включённым Telegram и этим прокси под `tokio::time::timeout(2s)`; ждёт возврата меньше чем за 2 с.
- RED: `docs/fix_logs/bug-88-red.log`, red_kind=assert; `notify_failover не должен блокировать failover на время доставки (ждали 2.0052836s)`
- Исправление (что и почему, 1–3 предложения): `notify_failover` клонирует http-клиент, конфиг и строки и запускает доставку через `tokio::spawn` (новая приватная `deliver_failover_notification` — прежнее тело), сразу возвращаясь. Сигнатура не менялась, все 7 вызовов в failover.rs продолжают работать без правок.
- Какие инварианты затрагивает и почему не нарушены: Секреты не логируются (логика доставки не менялась); сетевых сбросов нет. Порядок уведомлений теперь не гарантирован строго (две фоновые задачи могут обогнать друг друга) — приемлемо, т.к. в каждом сообщении есть время.
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Детектор БС, Best-Ping, priority chain, автовозврат — все используют тот же вызов; их логика переключения теперь не задерживается сетью.
- GREEN: `docs/fix_logs/bug-88-green.log`; строка `=== GATE OK bug N | diff ... ===` скопирована из вывода:
  `=== GATE OK bug 88 | diff 62224b68f7c4 | лог docs/fix_logs/bug-88-green.log ===`
- Статус: FIXED

### Баг 97 — geo_override стирается при сбое DNS; fake-ip/частные адреса не фильтруются; процесс ipset на каждый IP
- Правила перечитаны: 2026-11-05 04:28:05 и 04:53:14 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/override_sync.rs:128` — `sync_geo_override`; `:17` — `resolve_domains`; `sync_ipset_family`
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
      let (v4, v6) = resolve_domains(domains).await;
      let total_ips = v4.len() + v6.len();
      ...
                                  if !ip.is_loopback() && !ip.is_unspecified() {
                                      v4_res.push(ip);
      ...
                  if (family == "inet" && !is_v6) || (family == "inet6" && is_v6) {
                      let _ = run_ipset(&["add", &tmp, t, "-exist"]).await;
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: Подпункты: 97a — при недоступном DNS resolve_domains возвращает пустые наборы, write_override_file_streaming пишет файл без блока, swap на пустой ipset → force-домены уходят в DIRECT; 97b — фильтр пропускал всё, кроме loopback/unspecified (fake-ip 198.18/15, RFC1918, CGNAT, link-local, ULA); 97c — по процессу `ipset add` на каждый адрес. Все три совпадают с кодом. На роутере не проверялось (ipset restore на Entware стандартный, но живой прогон не делался).
- Первопричина одним предложением: sync_geo_override доверял любому результату резолвинга, включая пустой и служебные адреса, и заполнял ipset поштучно.
- Тест: `override_sync.rs::tests::regression_bug_97a_dns_outage_keeps_previous_override`, `..._97b_fake_ip_and_private_addresses_not_added`, `..._97c_ipset_filled_by_single_restore_batch`; spec: `be:regression_bug_97a / 97b / 97c`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): 97b вызывает рабочий `resolve_domains` с литералами IP (lookup_host отдаёт их без DNS) и ждёт только публичные 8.8.8.8 и 2001:4860:4860::8888. 97a проверяет `keep_previous_override` (её вызывает sync_geo_override перед записью). 97c проверяет `build_ipset_restore_script` (его вызывает sync_ipset_family), включая отбрасывание мусорных строк и чужого семейства.
- RED: 97b: `docs/fix_logs/bug-97b-red.log`, red_kind=assert (в наборе оказались 10.1.2.3, 198.18.0.5 и др.). 97a и 97c: red_kind=compile — функций ещё не было; это НЕ доказательство RED, ограничение: поведение «до» (стирание блока, поштучный add) видно из цитаты кода, но тестом не воспроизведено, т.к. sync_geo_override завязан на /opt и ipset.
- Исправление (что и почему, 1–3 предложения): Добавлены `is_override_eligible_v4/v6` (отсев fake-ip, частных, CGNAT, link-local, ULA, mapped), `resolve_domains_counted` со счётом разрешившихся доменов и `keep_previous_override`: если разрешилось меньше половины доменов, sync_geo_override возвращает Err и не трогает файл и ipset. Заполнение staging-ipset — одним `ipset restore -exist` из валидированного скрипта.
- Какие инварианты затрагивает и почему не нарушены: Zero-Touch сохраняется (при очистке всех доменов блок по-прежнему удаляется: requested=0). Никакого нового хардкода. Атомарный swap сохранён.
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Вызывают сохранение доменов, гейминг, бот, watchdog и старт: при сбое DNS они получают Err и логируют предупреждение, старые IP остаются. Игровые/CDN-домены идут тем же путём.
- GREEN по подпунктам; строки скопированы из вывода шлюза:
  `=== GATE OK bug 97a | diff d1dcc867d56f | лог docs/fix_logs/bug-97a-green.log ===`
  `=== GATE OK bug 97b | diff df3e87c878e0 | лог docs/fix_logs/bug-97b-green.log ===`
  `=== GATE OK bug 97c | diff df3e87c878e0 | лог docs/fix_logs/bug-97c-green.log ===`
- Статус: FIXED

### Баг 73 — Квота подписки списывается всем трафиком Mihomo
- Правила перечитаны: 2026-11-05 04:48:09 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/traffic.rs:390` — `run_mihomo_stream`; `add_provider_live_delta`
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
                                          let bytes = msg.down + msg.up;
                                          ...
                                          if let Some(act_prov) = get_active_provider_id() {
                                              add_provider_live_delta(&act_prov, bytes);
                                          }
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: Совпадает: весь /traffic (включая DIRECT, гейминг через другую подписку, ручные узлы) списывался с подписки активного сервера. Взят вместе с 72, т.к. путь один. Сделан раньше своей фазы F из-за общей правки с 72.
- Первопричина одним предложением: Квота считалась по суммарному потоку /traffic без разбора, через какой узел идёт соединение.
- Тест: `traffic.rs::tests::regression_bug_73_subscription_quota_counts_only_its_nodes`; spec: `be:regression_bug_73`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): Вызывает `split_connections_delta_with_providers` (её вызывает новый опросчик `run_connections_accounting`) с соединениями через узлы двух подписок, DIRECT и ручной узел; ждёт разнесения квоты по подпискам, только приростов во втором опросе, очистки закрытых соединений и счёта в БС при whitelist_active.
- RED: `docs/fix_logs/bug-73-red.log`, red_kind=compile. Ограничение: это не доказательство RED — до исправления существовала только заглушка, а старая логика жила внутри сетевого цикла run_mihomo_stream и тестом не воспроизводится.
- Исправление (что и почему, 1–3 предложения): Байты и квоты теперь считает отдельная задача `run_connections_accounting` (GET /connections раз в 2 с, карта «узел → подписка» из /providers/proxies раз в 30 с); `run_mihomo_stream` оставлен только для скоростей, масштабированных на долю не-DIRECT трафика.
- Какие инварианты затрагивает и почему не нарушены: Два канала учёта не смешиваются; секреты Mihomo только в заголовке Authorization; хардкода нет (URL из конфига).
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): БС: при WHITELIST_ACTIVE весь не-DIRECT трафик идёт в счётчик БС. Гейминг/Google AI через узлы подписок списываются со своих подписок. Нагрузка: +1 запрос /connections к локальному Mihomo раз в 2 с — на роутере не измерялась.
- GREEN: `docs/fix_logs/bug-73-green.log`; строка `=== GATE OK bug N | diff ... ===` скопирована из вывода:
  `=== GATE OK bug 73 | diff df3e87c878e0 | лог docs/fix_logs/bug-73-green.log ===`
- Статус: FIXED

### Баг 102 — Проверка WAN ждёт код 200 от адресов, отвечающих 204
- Правила перечитаны: 2026-11-05 04:57:33 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/failover.rs:1865` — `is_wan_online`; `backend/src/mihomo.rs:1501` — `ping_server_url_expected`
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
      for url in WAN_CHECK_URLS {
          let ms = mihomo::ping_server_url_expected(
              http,
              cfg,
              "DIRECT",
              1500,
              Some(url),
              Some(200),
          ).await;
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: Совпадает: для всех трёх проб (ya.ru, cp.cloudflare.com/generate_204, gstatic generate_204) передавался expected=200, а 204-эндпоинты отвечают 204, т.е. Mihomo считает их неуспешными. [?] из аудита (поведение ya.ru) на роутере не проверялось; поэтому для ya.ru допускается диапазон 200-399 (ответ или редирект на https).
- Первопричина одним предложением: Один фиксированный expected=200 для эндпоинтов с разными штатными кодами ответа.
- Тест: `failover.rs::tests::regression_bug_102_wan_check_accepts_204_endpoints`; spec: `be:regression_bug_102`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): Локальный мок Mihomo: /proxies/DIRECT/delay отвечает delay только для generate_204 с expected=204, ya.ru «лежит». Вызывается рабочий `is_wan_online`; ждём true.
- RED: `docs/fix_logs/bug-102-red.log`, red_kind=assert; `WAN жив: generate_204 отвечает 204, проверка не должна требовать 200`
- Исправление (что и почему, 1–3 предложения): `is_wan_online` берёт ожидаемый статус из `wan_check_expected_status(url)`: "204" для generate_204, "200-399" для ya.ru. Добавлен `ping_server_url_expected_range` с expected в формате Mihomo; старый `ping_server_url_expected(u16)` стал обёрткой — остальные вызовы не менялись.
- Какие инварианты затрагивает и почему не нарушены: Хардкода не прибавилось (URL уже были константами); сбросов соединений нет.
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Влияет на решение failover «WAN down → не переключать». Теперь при недоступном ya.ru WAN не считается упавшим, и failover работает. Проверено только с моком, не на роутере.
- GREEN: `docs/fix_logs/bug-102-green.log`; строка `=== GATE OK bug N | diff ... ===` скопирована из вывода:
  `=== GATE OK bug 102 | diff 3c28cbcfc96d | лог docs/fix_logs/bug-102-green.log ===`
- Статус: FIXED

### Баг 103 — Устаревшая цепочка приоритетов блокирует failover полностью
- Правила перечитаны: 2026-11-05 05:00:00 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/failover.rs:3259` — `run_check`, ветка падения активного сервера
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
          let has_priority_chain = !cfg.failover.priority_chain.is_empty() || !chain.is_empty();
  
          let chain_candidates: Vec<String> = if has_priority_chain {
              chain
                  .iter()
                  .filter(|s| s.id != active.id)
                  .map(|s| s.id.clone())
                  .collect()
          } else {
              Vec::new()
          };
  
          let target_res = if has_priority_chain {
              if chain_candidates.is_empty() {
                  Err("Все серверы цепочки приоритетов недоступны (в таймауте или карантине). Переключение заблокировано.".to_string())
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: Совпадает: при устаревших ID (или если в цепочке остался только активный сервер) `chain_candidates` пуст, но `has_priority_chain` истинно из-за непустого конфига → немедленная ошибка без рассмотрения остальных серверов. Строгая граница цепочки при ЖИВЫХ, но упавших серверах цепочки сохранена — это задуманное поведение, аудит его не оспаривает.
- Первопричина одним предложением: Признак «цепочка настроена» брался из конфига, а не из того, есть ли в ней существующие сейчас кандидаты.
- Тест: `failover.rs::tests::regression_bug_103_stale_priority_chain_does_not_block_failover`; spec: `be:regression_bug_103`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): Проверяет `priority_chain_candidates` (её вызывает run_check, и `has_priority_chain = !candidates.is_empty()`): устаревшие ID и цепочка из одного активного дают пустой список → best-of-rest; живые ID сохраняют порядок без активного.
- RED: `docs/fix_logs/bug-103-red.log`, red_kind=compile. Ограничение: не доказательство RED — старая логика была встроена в run_check, полноценный сценарий требует мока Mihomo со всем циклом run_check; поведение «до» видно из цитаты.
- Исправление (что и почему, 1–3 предложения): Вынесена `priority_chain_candidates`, `has_priority_chain` теперь равен наличию кандидатов; при непустом, но неприменимом конфиге в журнал failover пишется предупреждение, и кандидат выбирается из остальных серверов.
- Какие инварианты затрагивает и почему не нарушены: Два канала: best-of-rest использует ту же tiered-логику с is_whitelist_server_full, т.е. smart failover сохраняет порядок VPS → БС. Хардкода нет.
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Автовозврат на приоритетный сервер по-прежнему работает только по живым ID цепочки; детектор БС вызывается раньше этой ветки и не затронут.
- GREEN: `docs/fix_logs/bug-103-green.log`; строка `=== GATE OK bug N | diff ... ===` скопирована из вывода:
  `=== GATE OK bug 103 | diff e9ca955cd9fc | лог docs/fix_logs/bug-103-green.log ===`
- Статус: FIXED

### Баг 114 — «Лечение» AdGuard на каждом тике дописывает 5 публичных upstream
- Правила перечитаны: 2026-11-05 05:01:58 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/adguard.rs:327` — `clean_adguard_upstreams`, `:377` — `clean_adguard_bootstrap_dns`, `:1993` — `sanitize_and_heal_adguard_yaml`; вызывается из `heal_adguard_live_config` (watchdog.rs:821)
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
      let fast_defaults = [
          "77.88.8.8",
          "77.88.8.1",
          "1.1.1.1",
          "8.8.8.8",
          "https://dns.google/dns-query",
      ];
      for def in &fast_defaults {
          if !cleaned.iter().any(|u| u == *def) {
              cleaned.push(def.to_string());
              modified = true;
          }
      }
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: Совпадает: и Live API (через clean_adguard_upstreams), и YAML (если в файле нет строки 77.88.8.8) получали публичные резолверы при любом пользовательском списке; modified=true на каждом тике → лишние POST в AGH. Bootstrap — аналогично (77.88.8.8, 1.1.1.1).
- Первопричина одним предложением: Запасные резолверы добавлялись безусловно, а не только когда после удаления петель список пуст.
- Тест: `adguard.rs::tests::regression_bug_114_user_upstreams_are_not_extended_with_public_dns`; spec: `be:regression_bug_114`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): Вызывает рабочие `clean_adguard_upstreams`, `clean_adguard_bootstrap_dns` и `sanitize_and_heal_adguard_yaml` с DoH-only конфигом пользователя: список не меняется, modified=false, в YAML нет 77.88.8.8 и 8.8.8.8; при списке из одной петли — запасной резолвер есть.
- RED: `docs/fix_logs/bug-114-red.log`, red_kind=assert; `assertion left == right failed: рабочий список пользователя не дополняется`
- Исправление (что и почему, 1–3 предложения): Добавлена `FALLBACK_UPSTREAMS` (77.88.8.8, 77.88.8.1 — доступны и при БС); она подставляется только в пустой после очистки список (upstream, bootstrap, YAML-блок upstream_dns). Три старых теста закрепляли баг (`test_anti_regression_clean_adguard_upstreams` ждал 77.88.8.8/1.1.1.1/8.8.8.8 рядом с quad9, bootstrap-тест — 77.88.8.8/1.1.1.1 рядом с 9.9.9.9, YAML-тест — 77.88.8.8 при оставшемся 8.8.8.8); их проверки заменены на точное равенство ожидаемому списку — это строже, а не слабее.
- Какие инварианты затрагивает и почему не нарушены: AdGuard toggle ⇔ iptables не затронут. Хардкод публичных DNS сократился до одной константы запасных резолверов.
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): БС: запасные резолверы доступны при белых списках. Удаление петель 1053/53/7874 и адреса роутера работает как раньше.
- GREEN: `docs/fix_logs/bug-114-green.log`; строка `=== GATE OK bug N | diff ... ===` скопирована из вывода:
  `=== GATE OK bug 114 | diff e319a08ed67b | лог docs/fix_logs/bug-114-green.log ===`
- Статус: FIXED

### Баг 116 — Watchdog сам включает интеграцию AdGuard и adblock_enabled
- Правила перечитаны: 2026-11-05 05:04:51 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/watchdog.rs:801` — цикл AdGuard watchdog; переключатели `api.rs:5129` (toggle adblock), `api.rs:11641` (защита AGH), `bot.rs:1424`
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
              let needs_cfg_sync = is_running_protected && !enabled;
              if needs_cfg_sync {
                  enabled = true;
              }
              if needs_cfg_sync && !mihomo_down {
                  if let Ok(mut tx) = crate::transaction::ConfigTx::begin(&state).await {
                      tx.config_mut().adguard.enabled = true;
                      tx.config_mut().adblock_enabled = true;
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: Совпадает: пользователь выключает интеграцию в панели (api.rs:5129), AGH продолжает работать с защитой → на следующем тике сторож включает обе настройки обратно. Подпункт «две функции переключаются вместе» — в API и сторожем adblock_enabled и adguard.enabled ставятся парой как одна функция; раздельной семантики в UI нет, поэтому разделение не делалось — устранена именно невозможность выключить.
- Первопричина одним предложением: Условие автоподхвата не учитывало, что пользователь уже сделал выбор.
- Тест: `watchdog.rs::tests::regression_bug_116_watchdog_respects_user_adguard_choice`; spec: `be:regression_bug_116`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): Проверяет `should_auto_integrate_adguard` (её вызывает цикл watchdog вместо прежнего условия) и значение по умолчанию нового поля: первый подхват разрешён, после выбора пользователя — нет.
- RED: `docs/fix_logs/bug-116-red.log`, red_kind=compile. Ограничение: не доказательство RED — условие было встроено в сетевой цикл watchdog; поведение «до» видно из цитаты.
- Исправление (что и почему, 1–3 предложения): Новое поле `adguard.integration_choice_locked` (serde default false). Сторож подхватывает работающий AGH только при незафиксированном выборе и сразу фиксирует его; все явные переключатели (панель, защита AGH, бот) тоже фиксируют выбор.
- Какие инварианты затрагивает и почему не нарушены: AdGuard toggle ⇔ iptables: при выключенной интеграции и работающем AGH ветка `!enabled && !is_running_protected` по-прежнему снимает редирект только при мёртвом AGH; при отключённой интеграции сторож больше не возвращает редирект сам. Zero-Touch для внешнего AGH сохранён (первый подхват).
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Обратная совместимость: в старых config.json поля нет → false → один автоподхват, как раньше. Бот и панель ведут себя одинаково.
- GREEN: `docs/fix_logs/bug-116-green.log`; строка `=== GATE OK bug N | diff ... ===` скопирована из вывода:
  `=== GATE OK bug 116 | diff bc31f3e2f79e | лог docs/fix_logs/bug-116-green.log ===`
- Статус: FIXED

### Баг 133 — Сохранение настроек интеграции AdGuard отправляет и применяет полный старый снимок
- Правила перечитаны: 2026-11-05 05:07:25 и 05:10:03 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `frontend/src/components/AdGuard.tsx:305` — `handleSaveSettings`; `backend/src/api.rs:11813` — `adguard_set_config`
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
        await apiPost('/adguard/config', settingsForm)
  ...
      Json(mut new_adguard): Json<crate::config::AdGuardConfig>,
  ...
      current.adguard = new_adguard;
      tx.stage_json(current);
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: Совпадает: settingsForm — копия конфига на момент загрузки страницы, содержит enabled и dns_port; бэкенд заменял config.adguard целиком и при «изменении» enabled/port пересобирал маршруты и iptables на старый порт. Подпункты: 133a — фронт шлёт снимок; 133b — бэкенд заменяет целиком.
- Первопричина одним предложением: Форма и обработчик работали полным объектом вместо изменённых полей.
- Тест: `AdGuard.test.tsx` → `bug 133: saving integration settings sends only changed fields...`; `api.rs::settings_and_service_regression_tests::regression_bug_133_adguard_config_patch_keeps_background_state`; spec: `fe:src/components/AdGuard.test.tsx::bug 133: / be:regression_bug_133`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): 133a: рендер рабочего AdGuard.tsx, вкладка настроек, меняется только хост, submit формы → тело POST ровно {host}. 133b: `merge_adguard_config_patch` (её вызывает adguard_set_config) — сохраняет enabled, отсутствующий dns_port, флаг выбора, пароль при маске; явная смена порта применяется; не-объект отклоняется.
- RED: 133a: `docs/fix_logs/bug-133a-red.log`, red_kind=assert (`expected { enabled: true, …(9) } to deeply equal { host: '10.0.0.5' }`). 133b: red_kind=compile — функции не было; ограничение: замена целиком видна из цитаты, тестом не воспроизведена.
- Исправление (что и почему, 1–3 предложения): Фронт: `diffSettings(config, settingsForm)` без `enabled`, при отсутствии изменений — «Изменений нет». Бэкенд: обработчик принимает JSON-патч и мержит его поверх текущего config.adguard, игнорируя enabled/integration_choice_locked и маску пароля.
- Какие инварианты затрагивает и почему не нарушены: INV-5 (UI не шлёт полный снимок) и INV-2 (редирект 53→порт AGH) — порт меняется только явно; пересборка маршрутов и iptables по-прежнему при реальной смене порта.
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Переключатели AdGuard/AdBlock (отдельные API) не затронуты; сторож (116) остаётся единственным, кто подхватывает найденный порт.
- GREEN по подпунктам; строки скопированы из вывода шлюза:
  `=== GATE OK bug 133a | diff e1cdd58ffd8c | лог docs/fix_logs/bug-133a-green.log ===`
  `=== GATE OK bug 133b | diff e1cdd58ffd8c | лог docs/fix_logs/bug-133b-green.log ===`
- Статус: FIXED

### Баг 28 — Когда Zapret падает, YouTube и Discord перестают работать у всей сети
- Правила перечитаны: 2026-11-05 05:12:55 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/watchdog.rs:638` — ветка `restart_failures == 5` сторожа Zapret
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
                      let mut cfg = (**state.config.read().await).clone();
                      cfg.zapret.enabled = false;
                      let _ = config::save(&state.config_path, &cfg).await;
                      *state.config.write().await = std::sync::Arc::new(cfg);
                      // Z-03: маркер предотвращает перезапуск S51zapret
                      crate::zapret::set_zapret_disabled_marker();
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: Совпадает: после 5 неудачных стартов меняется только config.json/AppState; config.yaml с блоком AUTO-ZAPRET-HYBRID (YouTube/Discord → DIRECT) не пересобирается, Mihomo не перезагружается. Попутно закрыт сценарий бага 95 (запись без config_lock) именно в этой ветке; остальные места бага 95 будут проверены отдельно.
- Первопричина одним предложением: Выключение Zapret сторожем не проходило через ConfigTx с пересборкой маршрутов.
- Тест: `watchdog.rs::tests::regression_bug_28_zapret_give_up_rebuilds_mihomo_rules`; spec: `be:regression_bug_28`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): Во временном каталоге строит config.yaml через рабочий apply_routing с включённым Zapret (предпосылка: блок есть), вызывает `disable_zapret_after_failed_starts` (её вызывает сторож) с недоступным Mihomo и проверяет: блок Zapret удалён из YAML, zapret.enabled=false в config.json и в AppState.
- RED: `docs/fix_logs/bug-28-red.log`, red_kind=compile. Ограничение: не доказательство RED — исходная ветка встроена в бесконечный цикл сторожа; поведение «до» видно из цитаты.
- Исправление (что и почему, 1–3 предложения): Новая `disable_zapret_after_failed_starts`: ConfigTx (config_lock → routing_lock), zapret.enabled=false, apply_routing, commit_and_reload; если ядро не приняло reload — повторная транзакция с commit_without_reload, чтобы файлы были согласованы.
- Какие инварианты затрагивает и почему не нарушены: INV-3: запись YAML/JSON только через ConfigTx. Два канала не затронуты.
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Маркер Z-03 и stop-fw вызываются как раньше. Гейминг/БС правила пересобираются apply_routing тем же способом, что и при ручных изменениях.
- GREEN: `docs/fix_logs/bug-28-green.log`; строка `=== GATE OK bug N | diff ... ===` скопирована из вывода:
  `=== GATE OK bug 28 | diff f97e6c9b7fd9 | лог docs/fix_logs/bug-28-green.log ===`
- Статус: FIXED

### Баг 31 — DPI-инструмент (Zapret) меняет DNS всей сети; флаг ADGUARD_ENABLED; удаление чужих правил 1053
- Правила перечитаны: 2026-11-05 05:16:21 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/api.rs` — скрипт S51ZAPRET_SCRIPT, функция `add_fw` (редирект br+/Bridge+ :53 → 1053), `remove_fw_rules`; запись zapret.conf в `sync_zapret_files` (флаг ADGUARD_ENABLED); `backend/src/zapret.rs:249` — `DNS_REDIRECT_REMOVE_CMD`
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
      let adguard_enabled = std::path::Path::new("/opt/etc/xkeen-route/config.json")
          .exists()
          .then(|| crate::config::load(std::path::Path::new("/opt/etc/xkeen-route/config.json")).adguard.enabled)
          .unwrap_or(false)
          || std::path::Path::new("/opt/etc/init.d/S99adguardhome").exists()
          || crate::system::is_process_running("AdGuardHome");
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: Подпункты: 31a — флаг ADGUARD_ENABLED=1 при одном наличии S99adguardhome — FIXED (`=== GATE OK bug 31a | diff 296635833d1d | лог docs/fix_logs/bug-31a-green.log ===`, тест `regression_bug_31a_zapret_adguard_flag_ignores_mere_installation`, red_kind=compile — не доказательство RED). 31b — редирект DNS всей LAN на захардкоженный 1053 в add_fw и 31c — DNS_REDIRECT_REMOVE_CMD удаляет любые PREROUTING-правила с to-ports 1053 и не возвращает свои: НЕ исправлены, т.к. зависят от решения ниже.
- Первопричина одним предложением: Zapret-скрипт сам управляет DNS LAN, хотя это задача AdGuard/Mihomo; флаг AdGuard вычислялся по наличию файла.
- Тест: `api.rs::settings_and_service_regression_tests::regression_bug_31a_zapret_adguard_flag_ignores_mere_installation` (только 31a); spec: `be:regression_bug_31a`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): `zapret_conf_adguard_flag` (её вызывает запись zapret.conf): установленный, но выключенный и незапущенный AGH → 0.
- RED: 31a: red_kind=compile (ограничение — не доказательство RED). 31b/31c: не запускался.
- Исправление (что и почему, 1–3 предложения): 31a: флаг берётся из adguard.enabled || adblock_enabled в конфиге или из работающего процесса; наличие init-скрипта больше не учитывается.
- Какие инварианты затрагивает и почему не нарушены: INV-2 и INV-9 для 31b/31c не восстановлены — ждут решения.
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Zapret + AdGuard: при установленном, но выключенном AGH Zapret снова применяет свою DNS-логику (как при отсутствии AGH).
- GREEN: не запускался — BLOCKED
- Статус: BLOCKED (Вопрос пользователю: должен ли Zapret вообще перенаправлять DNS всей LAN на Mihomo, когда AdGuard выключен? Вариант 1 — убрать DNS-редирект из S51zapret полностью (DNS — зона AdGuard/Mihomo, Zapret только DPI); вариант 2 — оставить, но брать порт из dns.listen Mihomo и удалять только свои правила с комментарием xkeen-route-zapret. Рекомендую вариант 1: меньше пересечений с AdGuard/БС/геймингом и соблюдается «строго два канала»; риск — DNS-подмена провайдера для YouTube при выключенном AdGuard, проверить можно только на роутере.)

### Баг 56 — Каждый запуск Zapret из UI меняет системные DNS роутера
- Правила перечитаны: 2026-11-05 05:18:24 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/api.rs:8232` — `zapret_action` (ветка start/restart/start-fw/reload-fw); `backend/src/rci.rs:853` — `set_clean_dns_servers`
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
      if action_to_run == "start" || action_to_run == "restart" || action_to_run == "start-fw" || action_to_run == "reload-fw" {
          ...
          let _ = sync_zapret_files(&cfg.zapret).await;
          let _ = crate::rci::set_clean_dns_servers(&state.http, &cfg).await;
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: Совпадает: каждое из четырёх действий вызывало set_clean_dns_servers (77.88.8.8, 77.88.8.1, 1.1.1.1 + system configuration save). Других вызовов, кроме явного POST /api/dns/clean-servers, нет (grep по bot/watchdog/main/zapret пуст). Удаление ранее добавленных серверов из конфига Keenetic не делалось — это изменение настроек роутера, требует решения пользователя и проверки на роутере.
- Первопричина одним предложением: Служебное действие Zapret выполняло скрытую мутацию системных настроек роутера.
- Тест: `api.rs::settings_and_service_regression_tests::regression_bug_56_zapret_actions_do_not_touch_router_dns`; spec: `be:regression_bug_56`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): Структурный тест по исходнику (тем же `fn_body`, что и соседние тесты модуля): в теле `zapret_action` нет вызова set_clean_dns_servers, а явный обработчик `apply_clean_dns` его сохраняет. Ограничение: проверка по тексту, а не по поведению — RCI на роутере в тестах недоступен.
- RED: `docs/fix_logs/bug-56-red.log`, red_kind=assert; `запуск Zapret не должен менять системные DNS роутера`
- Исправление (что и почему, 1–3 предложения): Вызов set_clean_dns_servers убран из zapret_action; устаревший комментарий в пресетах исправлен. Явная кнопка «чистые DNS» осталась.
- Какие инварианты затрагивает и почему не нарушены: Нет скрытых изменений системных настроек; 1.1.1.1 при БС больше не подставляется автоматически.
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Zapret стартует как раньше (sync_zapret_files). Уже записанные ранее в Keenetic DNS остаются — пользователь может убрать их вручную в веб-интерфейсе роутера.
- GREEN: `docs/fix_logs/bug-56-green.log`; строка `=== GATE OK bug N | diff ... ===` скопирована из вывода:
  `=== GATE OK bug 56 | diff 0b7833894f8c | лог docs/fix_logs/bug-56-green.log ===`
- Статус: FIXED

### Баг 57 — Вкл/выкл Zapret из бота не пересобирает правила Mihomo; при неудачном старте остаётся enabled=true
- Правила перечитаны: 2026-11-05 05:21:27 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/api.rs:8194` — `zapret_set_enabled_core` (единственный вызов — `bot.rs:583`)
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
          let mut cfg = (**state.config.read().await).clone();
          cfg.zapret.enabled = enabled;
          sync_zapret_files(&cfg.zapret).await?;
          if let Err(e) = config::save(&state.config_path, &cfg).await {
              return Err(format!("Настройки Zapret не сохранены в config.json: {e}"));
          }
          *state.config.write().await = std::sync::Arc::new(cfg);
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: Совпадает по всем трём подпунктам: (а) правила Mihomo не пересобираются ни при включении, ни при выключении; (б) enabled=true сохраняется ДО старта, при ошибке старта не откатывается; (в) у UI своя реализация (zapret_action) — теперь бот и сторож используют общую `persist_zapret_enabled_with_routing`; zapret_action UI переведу на неё в рамках бага 58 (там же запись YAML без транзакции).
- Первопричина одним предложением: Ядро переключения писало только config.json и не проходило через ConfigTx + apply_routing.
- Тест: `watchdog.rs::tests::regression_bug_57_zapret_toggle_rebuilds_rules_both_ways`; spec: `be:regression_bug_57`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): Во временном каталоге вызывает `persist_zapret_enabled_with_routing` (её вызывают zapret_set_enabled_core и сторож): включение добавляет блок AUTO-ZAPRET-HYBRID в config.yaml и enabled=true в AppState, выключение убирает блок и пишет enabled=false в config.json.
- RED: `docs/fix_logs/bug-57-red.log`, red_kind=compile. Ограничение: не доказательство RED; порядок «старт → затем сохранение» проверен чтением кода, т.к. ядро требует /opt/etc/init.d/S51zapret.
- Исправление (что и почему, 1–3 предложения): zapret_set_enabled_core: файлы службы пишутся с целевым флагом, но enabled=true сохраняется только после успешного `S51zapret start` (с пересборкой правил); при выключении сначала транзакция с пересборкой, затем stop. Сторож (баг 28) теперь вызывает ту же функцию — дубликат удалён.
- Какие инварианты затрагивает и почему не нарушены: INV-3 (ConfigTx), INV-1 не затронут. При неуспешном reload — согласованные файлы без reload.
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Zapret + гейминг/БС: apply_routing пересобирает все блоки, как при ручных изменениях. Бот теперь ведёт себя как сторож.
- GREEN: `docs/fix_logs/bug-57-green.log`; строка `=== GATE OK bug N | diff ... ===` скопирована из вывода:
  `=== GATE OK bug 57 | diff 7a478aec41e5 | лог docs/fix_logs/bug-57-green.log ===`
- Статус: FIXED

### Баг 59 — «Режим паники» в боте не держится и не даёт 100% DIRECT
- Правила перечитаны: 2026-11-05 05:26:48 и 05:28:17 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/bot.rs:1115` — `handle_emergency_toggle`, `handle_emergency_resume`; `backend/src/watchdog.rs:26` — `set_service_stopped`
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
      *PRE_EMERGENCY_SERVER.lock().await = Some(active_node);
      let _ = crate::mihomo::switch_server(&state.http, &cfg, "DIRECT").await;
      let _ = crate::mihomo::close_all_connections(&state.http, &cfg).await;
  ...
      let target = PRE_EMERGENCY_SERVER.lock().await.take().unwrap_or_else(|| "Fastest".to_string());
      let _ = crate::mihomo::switch_server(&state.http, &cfg, &target).await;
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: Совпадает по подпунктам: (1) failover не знает о панике и возвращает VPS; (2) БС/гейминг/Google AI/группы устройств/XKEEN_BS не затрагиваются; (3) сохраняется имя, а не ID; (4) состояние только в памяти. 59a закрывает 1–3: паника = полная остановка служб панели (`xkeen_service stop`: снятие XKEEN_BS/гейминга/DNS-редиректов, флаг is_service_stopped, на который смотрят failover и сторожа), возобновление = штатный `start` (восстановление правил, туннелей и Zapret), поиск сервера по имени больше не нужен. 59c закрывает 4. Подпункт 59b не выделялся.
- Первопричина одним предложением: Бот имел собственную облегчённую «панику» вместо общей остановки служб, а флаг остановки жил только в памяти.
- Тест: `bot.rs::tests::regression_bug_59a_panic_mode_uses_full_service_stop`; `watchdog.rs::tests::regression_bug_59c_service_stopped_flag_survives_panel_restart`; spec: `be:regression_bug_59a / be:regression_bug_59c`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): 59a — структурный тест по исходнику бота: тела обработчиков вызывают `crate::api::xkeen_service`, не используют `switch_server(` и `PRE_EMERGENCY_SERVER` (ограничение: текстовая проверка, т.к. сами службы требуют роутер). 59c — `persist_service_stopped_flag`/`read_service_stopped_flag` (их вызывают set_service_stopped и init при старте).
- RED: 59a: `docs/fix_logs/bug-59a-red.log`, red_kind=assert (`паника = та же полная остановка служб, что и в панели`). 59c: red_kind=compile — не доказательство RED.
- Исправление (что и почему, 1–3 предложения): Обработчики паники бота вызывают `api::xkeen_service` со stop/start; статический PRE_EMERGENCY_SERVER удалён. Флаг «службы остановлены» пишется в файл во временном каталоге и читается при старте панели (переживает рестарт панели, но не перезагрузку роутера, после которой XKeen стартует сам).
- Какие инварианты затрагивает и почему не нарушены: INV-1: в панике один канал Direct. Сторожа и failover приостанавливаются существующим флагом.
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Поведение паники совпало с кнопкой «Остановить службы» в панели; пользователь может возобновить и из бота, и из панели.
- GREEN по подпунктам; строки скопированы из вывода шлюза:
  `=== GATE OK bug 59a | diff 6f2319bce44e | лог docs/fix_logs/bug-59a-green.log ===`
  `=== GATE OK bug 59c | diff 6f2319bce44e | лог docs/fix_logs/bug-59c-green.log ===`
- Статус: FIXED

### Баг 62 — В режиме Best-Ping нет возврата с БС на VPS
- Правила перечитаны: 2026-11-05 05:31:29 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/failover.rs:2933` — ветка `if cfg.failover.best_ping_enabled` в `run_check`; «Сценарий Б» — `failover.rs:3416`
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
          if is_active_healthy {
              ...
              let msg = format!(
                  "Best-Ping Failover: активный '{}' работает штатно (пинг {act_ping} мс <= порога {threshold} мс), переключение не требуется",
                  act_name
              );
              return Ok(msg);
          }
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: Совпадает: при включённом Best-Ping здоровый активный узел (в т.ч. [БС]) всегда завершал run_check до автовозврата; ни smart_failover (возврат с БС на VPS), ни auto_restore_priority не срабатывали. Полный сценарий с моком Mihomo и Canary не прогонялся — только решение и компиляция общего пути; на роутере не проверялось.
- Первопричина одним предложением: Ранний return в ветке Best-Ping при здоровом активном узле.
- Тест: `failover.rs::tests::regression_bug_62_best_ping_continues_to_auto_restore`; spec: `be:regression_bug_62`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): Проверяет `best_ping_should_continue_to_restore` (её вызывает ветка Best-Ping): БС + smart failover и живая цепочка + auto_restore → продолжаем к Сценарию Б; пустая цепочка или обычный VPS без цепочки → прежний ранний выход. Весь набор тестов failover (103) проходит.
- RED: `docs/fix_logs/bug-62-red.log`, red_kind=compile. Ограничение: не доказательство RED — ранний return встроен в сетевой run_check; поведение «до» видно из цитаты.
- Исправление (что и почему, 1–3 предложения): Ветка Best-Ping оформлена помеченным блоком; при здоровом узле и применимом автовозврате замеры (пинг, трафик, чёрная дыра) сохраняются в `best_ping_measured`, и выполнение идёт в общий путь, который использует их вместо повторного пинга и не учитывает трафик БС второй раз. Иначе — прежний ранний выход.
- Какие инварианты затрагивает и почему не нарушены: INV-1: возврат с БС на VPS идёт тем же Canary-механизмом Сценария Б. INV-7: лишних переключений нет — решение о сбое в общем пути принимается по тем же замерам.
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Best-Ping при сбое активного узла не изменился; детектор БС не затронут.
- GREEN: `docs/fix_logs/bug-62-green.log`; строка `=== GATE OK bug N | diff ... ===` скопирована из вывода:
  `=== GATE OK bug 62 | diff 8ded269f3e0a | лог docs/fix_logs/bug-62-green.log ===`
- Статус: FIXED

### Баг 27 — Утечка правил iptables: RETURN-правила Zapret в nat PREROUTING не удаляются
- Правила перечитаны: 2026-11-05 05:34:20 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/api.rs:6159` — `S51ZAPRET_SCRIPT`, функции `add_fw` и `remove_fw_rules`
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```sh
    if ! check_adguard_active && check_dns_resolver_ready; then
      iptables -t nat -A PREROUTING -i br+ -d 127.0.0.1 -j RETURN 2>/dev/null || true
      iptables -t nat -A PREROUTING -i Bridge+ -d 127.0.0.1 -j RETURN 2>/dev/null || true
      ...
          iptables -t nat -A PREROUTING -i br+ -s "$ex_ip" -p udp --dport 53 -j RETURN 2>/dev/null || true
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: Совпадает: в remove_fw_rules есть удаление REDIRECT 1053 и mangle-правил, но нет ни одного удаления nat RETURN; add_fw начинается с remove_fw_rules и снова добавляет RETURN → рост цепочки при каждом restart/reload-fw/NDM-хуке. Синтаксис итогового скрипта проверен `sh -n` (Git Bash), на роутере не запускался.
- Первопричина одним предложением: У RETURN-правил DNS-защиты не было парной очистки.
- Тест: `api.rs::settings_and_service_regression_tests::regression_bug_27_zapret_return_rules_are_cleaned_up`; spec: `be:regression_bug_27`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): Разбирает рабочую константу S51ZAPRET_SCRIPT (это и есть исполняемый на роутере код): remove_fw_rules удаляет старые RETURN без комментария и все свои по комментарию; каждое nat RETURN в add_fw ставится с комментарием xkeen-route-zapret (вариант без комментария — только как запасной сразу после).
- RED: `docs/fix_logs/bug-27-red.log`, red_kind=assert; `старые RETURN без комментария удаляются`
- Исправление (что и почему, 1–3 предложения): RETURN-правила получают комментарий `xkeen-route-zapret` (с запасным вариантом без него для ядер без модуля comment); remove_fw_rules удаляет все nat PREROUTING RETURN с этим комментарием через `iptables -S`, а также старые некомментированные для 127.0.0.1 и текущих EXCLUDED_IPS.
- Какие инварианты затрагивает и почему не нарушены: INV-6: парная очистка есть, повторные add_fw не растят цепочку. Чужие правила не трогаются — удаление по своему комментарию или по точному совпадению.
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Сам DNS-редирект Zapret (вопрос из бага 31) не менялся.
- GREEN: `docs/fix_logs/bug-27-green.log`; строка `=== GATE OK bug N | diff ... ===` скопирована из вывода:
  `=== GATE OK bug 27 | diff e2df54250b5e | лог docs/fix_logs/bug-27-green.log ===`
- Статус: FIXED

### Баг 38 — Включение одного устройства в БС включает БС на всю сеть; нет синхронизации iptables
- Правила перечитаны: 2026-11-05 05:37:45 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/api.rs:12425` — `toggle_whitelist_device`; `build_whitelist_full_tunnel_commands` (перехват 192.168/16, 10/8, 172.16/12); `routing.rs:2274–2300` (правила БС)
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
      if body.enabled && !tx.config().whitelist.enabled {
          tx.config_mut().whitelist.enabled = true;
      }
      ...
      if let Err(e) = tx.commit().await {
          return api_err(format!("Ошибка сохранения config.json: {e}"));
      }
      log_i!(
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: Подпункты: 38a — включение одного устройства ставит whitelist.enabled=true, а StrictXKeen перехватывает все приватные подсети; 38b — после коммита не синхронизировались iptables XKEEN_BS и туннель БС — FIXED (`=== GATE OK bug 38b | diff c04aa8955131 | лог docs/fix_logs/bug-38b-green.log ===`, red_kind=assert); 38c — выключение последнего устройства не выключает БС. По коду 38a/38c — следствие задуманного Zero-Touch (routing.rs 2.4: «Все клиенты локальных сетей → в туннель БС»; whitelist.devices используются для закрепления узлов BS-PIN), поэтому это продуктовое решение, а не очевидная ошибка.
- Первопричина одним предложением: 38b: toggle_whitelist_device не вызывал синхронизацию рантайма, которую делает save_whitelist.
- Тест: `api.rs::settings_and_service_regression_tests::regression_bug_38b_whitelist_device_toggle_syncs_iptables` (только 38b); spec: `be:regression_bug_38b`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): Структурный тест по исходнику: toggle_whitelist_device и save_whitelist вызывают общую `sync_whitelist_runtime_after_commit` (туннель БС + sync_full_tunnel_modes). Ограничение: текстовая проверка, т.к. iptables требует роутер.
- RED: 38b: `docs/fix_logs/bug-38b-red.log`, red_kind=assert. 38a/38c: не запускался.
- Исправление (что и почему, 1–3 предложения): 38b: вынесена `sync_whitelist_runtime_after_commit` из save_whitelist и вызывается после коммита в toggle_whitelist_device.
- Какие инварианты затрагивает и почему не нарушены: INV-1 для 38a — зависит от решения ниже.
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Гейминг взаимно исключается с БС в sync_full_tunnel_modes, как и при сохранении настроек БС.
- GREEN: не запускался — BLOCKED
- Статус: BLOCKED (Вопрос пользователю: должен ли тумблер БС у одного устройства включать БС только для этого устройства? Вариант 1 — да: при непустом списке включённых whitelist.devices перехват (iptables и SRC-IP-CIDR) только по их IP, пустой список = вся LAN; выключение последнего устройства выключает БС, если его включил тумблер. Вариант 2 — оставить глобальный БС (Zero-Touch), а тумблер в UI переименовать в «закрепить узел БС». Рекомендую вариант 1 — так ожидает пользователь, нажимая тумблер у устройства.)

### Баг 109 — «Проверка DPI» в Zapret всегда показывает YouTube/Discord недоступными
- Правила перечитаны: 2026-11-05 05:40:41 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/api.rs:8729` — ветка `act == "test_dpi"` в `zapret_action`
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```sh
              d_yt=$(curl -4 -k -m 2.5 -s -o /dev/null -w "%{{http_code}}:%{{time_total}}" https://www.youtube.com/generate_204 2>/dev/null || echo "000:0.0") &
              ...
              wait
              echo "$d_yt#$d_dc#$p_yt#$p_dc"
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: Совпадает: присваивание с `&` выполняется в фоновом подшелле, родитель после wait видит пустые переменные → вывод `###` → все коды 0 → «недоступно». Шаги: (1) сборка скрипта вынесена в `build_dpi_test_script` без изменения текста (рефакторинг), (2) тест запущен на этом старом тексте — RED, (3) исправление.
- Первопричина одним предложением: Результаты фоновых curl сохранялись в переменные подшеллов.
- Тест: `api.rs::settings_and_service_regression_tests::regression_bug_109_dpi_test_script_keeps_background_results`; spec: `be:regression_bug_109`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): Запускает рабочий скрипт из `build_dpi_test_script` в настоящем `sh` с подменённой функцией curl (без сети) и ждёт `204:0.10#200:0.20#204:0.10#200:0.20`. Требует sh (есть в Git Bash и на ubuntu-runner CI).
- RED: `docs/fix_logs/bug-109-red.log`, red_kind=assert; `left: "###"` — поведенческое воспроизведение бага на старом тексте скрипта.
- Исправление (что и почему, 1–3 предложения): Каждая проба `probe` пишет результат в свой файл во временном каталоге (mktemp -d), после wait файлы читаются и каталог удаляется; пустой ответ curl заменяется на 000:0.0.
- Какие инварианты затрагивает и почему не нарушены: Прокси-URL по-прежнему очищается от кавычек/метасимволов (Z-13) до подстановки.
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Только диагностическая кнопка; на правила маршрутизации не влияет.
- GREEN: `docs/fix_logs/bug-109-green.log`; строка `=== GATE OK bug N | diff ... ===` скопирована из вывода:
  `=== GATE OK bug 109 | diff d8dcf7d61cef | лог docs/fix_logs/bug-109-green.log ===`
- Статус: FIXED

### Баг 135 — Игровой режим с вкладки «Устройства» создаёт игровое устройство без IP
- Правила перечитаны: 2026-11-05 05:43:47 и 05:44:55 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `frontend/src/components/Devices.tsx:506` — `handleToggleGaming`; `backend/src/api.rs:11620` — `toggle_gaming`
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
        await apiPost('gaming/toggle', {
          enabled,
          device_mac: mac,
        })
  ...
                  tx.config_mut().gaming.devices.push(config::GamingDevice {
                      mac: mac_trimmed,
                      ip: String::new(),
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: Совпадает: UI отправлял только MAC, бэкенд создавал устройство с пустым IP. Подпункты: 135a — UI шлёт IP/имя; 135b — бэкенд берёт IP из запроса или ищет по MAC через RCI и отказывает, если IP не найден. Старый тест Devices.test.tsx (строка 158) ожидал payload только с MAC — он закреплял баг и обновлён.
- Первопричина одним предложением: Ни UI, ни бэкенд не обеспечивали IP для нового игрового устройства, а правила строятся по IP.
- Тест: `Devices.test.tsx` → `bug 135: enabling gaming from Devices tab sends device IP and name, not only MAC`; `api.rs::settings_and_service_regression_tests::regression_bug_135_gaming_device_requires_ip`; spec: `fe:src/components/Devices.test.tsx::bug 135: / be:regression_bug_135`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): 135a: рендер рабочего Devices.tsx, клик по игровой кнопке → POST gaming/toggle с device_ip и device_name. 135b: `upsert_gaming_device` (её вызывает toggle_gaming): без IP новое устройство не создаётся (ошибка), с IP — создаётся, IP обновляется, выключение без IP работает, инъекция в IP отклоняется.
- RED: 135a: `docs/fix_logs/bug-135a-red.log`, red_kind=assert. 135b: red_kind=compile — не доказательство RED.
- Исправление (что и почему, 1–3 предложения): UI добавляет device_ip/device_name; ToggleGamingReq получил эти поля; toggle_gaming при отсутствии IP ищет его по MAC через RCI, затем `upsert_gaming_device` валидирует IP и отказывает с понятной ошибкой вместо создания «пустого» устройства.
- Какие инварианты затрагивает и почему не нарушены: Валидация IP (is_valid_ip_or_cidr) до попадания в iptables-команды. Zero-Touch: IP находится автоматически, если устройство в сети.
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Режимы гейминга (Compatibility/KnownServices) получают реальный IP; DHCP-сторож продолжает обновлять IP в Compatibility.
- GREEN по подпунктам; строки скопированы из вывода шлюза:
  `=== GATE OK bug 135a | diff 0bb9f36ec57c | лог docs/fix_logs/bug-135a-green.log ===`
  `=== GATE OK bug 135b | diff 0bb9f36ec57c | лог docs/fix_logs/bug-135b-green.log ===`
- Статус: FIXED

### Баг 84 — Speedtest переключает всю LAN на тестовый сервер
- Правила перечитаны: 2026-11-05 05:49:13 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/speedtest.rs:151` — `run_speedtest`; `backend/src/routing.rs:3259` — условие добавления Canary Probe
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
      // 1. Создание снимка текущего выбора всех групп Selector ядра Mihomo
      let (group_snapshot, original_server) = take_groups_snapshot(_http, _cfg).await?;
      ...
          match crate::mihomo::switch_server(_http, _cfg, target_server).await {
      ...
      let proxy_url = _cfg.mihomo_proxy_url();
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: Совпадает: switch_server переключает основные selector-группы (в т.ч. в режиме БС — LAN на VPS → офлайн), замер идёт через mixed-port по правилам. Попутно на этом пути устранены: откат всех групп из снимка (баг 85 — теперь снимок и откат только Canary Probe) и замер «по правилам» (баг 86 — скачивание через слушатель выбранного узла); 85/86 будут отмечены отдельно после своих проверок (хардкод групп в 86 вне этого пути). На роутере не проверялось.
- Первопричина одним предложением: Speedtest использовал глобальное переключение сервера вместо изолированного канала.
- Тест: `speedtest.rs::tests::regression_bug_84_speedtest_switches_only_isolated_probe_group`; spec: `be:regression_bug_84`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): `isolated_speedtest_snapshot` (её вызывает run_speedtest): из набора PROXY / 🛡️ Whitelist / Canary Probe снимок и откат содержат только Canary Probe; без этой группы — отказ с понятным сообщением вместо переключения LAN.
- RED: `docs/fix_logs/bug-84-red.log`, red_kind=compile. Ограничение: не доказательство RED — старый путь сетевой (switch_server + get_proxies с глобальным кэшем); поведенческий тест с моком Mihomo не делался из-за общего кэша прокси между параллельными тестами.
- Исправление (что и почему, 1–3 предложения): run_speedtest переключает только `🛡️ Canary Probe` (switch_selector), подтверждает выбор по этой группе, качает через 127.0.0.1:CANARY_PROBE_PORT и восстанавливает только её. apply_routing добавляет Canary Probe всегда (раньше только при включённом failover). Старый тест `test_apply_routing_with_failover_enabled_injects_canary_and_cleans_when_disabled` ожидал удаление группы без failover — ожидание изменено на «группа остаётся и не дублируется», т.к. изменилось требование.
- Какие инварианты затрагивает и почему не нарушены: INV-1: LAN во время замера остаётся на своём канале (Direct + БС или VPS). INV-7: соединения LAN не рвутся.
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Failover на время speedtest приостановлен speedtest_lock, поэтому за группу Canary они не конкурируют. Бот-speedtest без lock (баг 129) — отдельный пункт.
- GREEN: `docs/fix_logs/bug-84-green.log`; строка `=== GATE OK bug N | diff ... ===` скопирована из вывода:
  `=== GATE OK bug 84 | diff 26b7d38a71d5 | лог docs/fix_logs/bug-84-green.log ===`
- Статус: FIXED

### Баг 118 — DHCP-слежение есть только для игровых устройств в Compatibility/BypassRu
- Правила перечитаны: 2026-11-05 05:52:41 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/watchdog.rs:398` — `spawn_dhcp_device_monitor`
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
              let (gaming_enabled, is_compat, has_devices) = {
                  let cfg = state.config.read().await;
                  (
                      cfg.gaming.enabled,
                      cfg.gaming.mode == config::GamingMode::Compatibility || cfg.gaming.mode == config::GamingMode::BypassRu,
                      !cfg.gaming.devices.is_empty(),
                  )
              };
  
              if gaming_enabled && is_compat && has_devices {
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: Совпадает: монитор работал только при гейминге Compatibility/BypassRu и обновлял только gaming.devices. device_routing, device_domain_rules, whitelist.devices/excluded_devices, zapret.excluded_devices и schedules не переносились. Попутно этот же монитор писал YAML/JSON в обход ConfigTx (баг 96) — теперь через ConfigTx. Ограничение: после рестарта панели карта MAC→IP начинается с текущих адресов — смена IP во время простоя панели не отслеживается.
- Первопричина одним предложением: Слежение за DHCP было частным случаем гейминга, а остальные настройки привязаны к IP без перевязки.
- Тест: `watchdog.rs::tests::regression_bug_118_ip_keyed_settings_follow_device_after_dhcp_change`; spec: `be:regression_bug_118`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): `detect_dhcp_ip_changes` и `migrate_ip_keyed_config` (их вызывает монитор на каждом тике): смена IP одного MAC даёт одну пару, новое устройство на старом IP — не «изменение»; все IP-привязанные настройки переезжают на новый адрес, MAC-исключения не трогаются.
- RED: `docs/fix_logs/bug-118-red.log`, red_kind=compile. Ограничение: не доказательство RED — старая логика в бесконечном цикле с RCI; поведение «до» видно из цитаты.
- Исправление (что и почему, 1–3 предложения): Монитор всегда опрашивает RCI, хранит MAC→IP, при смене IP переносит IP-привязанные настройки, затем (при гейминге) прежняя логика игровых устройств; запись через ConfigTx + commit_and_reload, затем sync_full_tunnel_modes и перезапись zapret.conf при наличии исключений.
- Какие инварианты затрагивает и почему не нарушены: INV-3: запись только через ConfigTx (закрывает часть бага 96 для этого монитора). Без лишних сбросов: reload только при реальных изменениях.
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Гейминг, БС по устройствам, Zapret-исключения, расписания и персональные маршруты — все следуют за устройством.
- GREEN: `docs/fix_logs/bug-118-green.log`; строка `=== GATE OK bug N | diff ... ===` скопирована из вывода:
  `=== GATE OK bug 118 | diff 4027a47e2843 | лог docs/fix_logs/bug-118-green.log ===`
- Статус: FIXED

### Баг 130 — Повреждённый config.json → все настройки заменяются дефолтами
- Правила перечитаны: 2026-11-05 05:56:05 и 05:57:33 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/config.rs:1311` — `load`; `:1272` — `parse_config_content`; `main.rs:405` — загрузка при старте
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
                  Err(err) => {
                      eprintln!("[ERROR] Поврежденный файл конфигурации {}: {err}", path.display());
                      ...
                      parse_config_content(&content, &path.display().to_string())
                  }
  ...
              // Пытаемся восстановить секцию auth, чтобы ни при каких условиях не отключить авторизацию
              let mut fallback = AppConfig::default();
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: Совпадает: при ошибке строгого разбора берётся AppConfig::default() с сохранением только auth; пустой файл (потеря питания) не содержит следов auth. Подпункты: 130a — восстановление из последней рабочей копии; 130b — копия обновляется при каждом успешном старте. Если копии нет (первый запуск после обновления) — прежнее поведение с сохранением auth.
- Первопричина одним предложением: Не было последней рабочей копии конфига, и аварийная загрузка падала на дефолты.
- Тест: `config.rs::tests::regression_bug_130a_corrupted_config_restores_last_good_backup`, `..._130b_startup_refreshes_backup_of_good_config`; spec: `be:regression_bug_130a / be:regression_bug_130b`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): 130a: рабочий `load` при пустом и обрезанном config.json и наличии config.json.bak возвращает настройки из копии. 130b: `load_and_refresh_backup` (её вызывает main при старте) создаёт копию из корректного файла и не затирает её повреждённым.
- RED: 130a: `docs/fix_logs/bug-130a-red.log`, red_kind=assert (`из последней рабочей копии, а не дефолты`). 130b: red_kind=compile — не доказательство RED.
- Исправление (что и почему, 1–3 предложения): Добавлены `backup_path`, `load_backup`, `write_file_synced`, `load_and_refresh_backup`; `load` при ошибке разбора или чтения сначала пробует config.json.bak; main использует load_and_refresh_backup.
- Какие инварианты затрагивает и почему не нарушены: Пароль не может пропасть: копия содержит auth. Запись копии — tmp + fsync + rename, раз за старт панели (износ флеша минимальный).
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Все режимы восстанавливаются вместе с конфигом из копии.
- GREEN по подпунктам; строки скопированы из вывода шлюза:
  `=== GATE OK bug 130a | diff 73ac23f518e2 | лог docs/fix_logs/bug-130a-green.log ===`
  `=== GATE OK bug 130b | diff 73ac23f518e2 | лог docs/fix_logs/bug-130b-green.log ===`
- Статус: FIXED

### Баг 94 — config::save без fsync
- Правила перечитаны: 2026-11-05 05:56:05 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/config.rs:1372` — `save` (через него пишут ConfigTx и ~50 мест)
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
      if let Err(e) = tokio::fs::write(&tmp, serialized).await {
          let _ = tokio::fs::remove_file(&tmp).await;
          return Err(e.to_string());
      }
      if let Err(e) = tokio::fs::rename(&tmp, path).await {
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: Совпадает: tmp-файл записывался без sync_all, затем rename — при потере питания возможен пустой config.json. Проверка на реальную потерю питания невозможна локально.
- Первопричина одним предложением: Отсутствие fsync перед rename (в отличие от atomic_write_file, Z-12).
- Тест: `config.rs::tests::regression_bug_94_config_save_fsyncs_before_rename`; spec: `be:regression_bug_94`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): Структурный тест по исходнику: в теле `save` есть sync_all и нет записи через tokio::fs::write без fsync. Ограничение: fsync не наблюдаем в юнит-тесте, проверяется только наличие вызова.
- RED: `docs/fix_logs/bug-94-red.log`, red_kind=assert; `данные файла должны быть сброшены на диск до rename`
- Исправление (что и почему, 1–3 предложения): save пишет через File + flush + sync_all, затем rename и (на Unix) fsync каталога. Вместе с 130 даёт восстановление из копии, если сбой всё же случился.
- Какие инварианты затрагивает и почему не нарушены: Атомарность rename сохранена; формат файла не менялся.
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Влияет только на надёжность записи конфигурации.
- GREEN: `docs/fix_logs/bug-94-green.log`; строка `=== GATE OK bug N | diff ... ===` скопирована из вывода:
  `=== GATE OK bug 94 | diff 73ac23f518e2 | лог docs/fix_logs/bug-94-green.log ===`
- Статус: FIXED

### Баг 76 — Конвертер подписок теряет cipher (SS) и cipher/alterId (VMess)
- Правила перечитаны: 2026-11-05 06:02:00 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/subscription_parser.rs:605` — `convert_subscription_to_mihomo_yaml` (вывод узлов)
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
          yaml.push_str(&format!("  - name: \"{}\"
  ", name.replace('"', "\\\"")));
          yaml.push_str(&format!("    type: {ptype}
  "));
          yaml.push_str(&format!("    server: \"{server}\"
  "));
          yaml.push_str(&format!("    port: {port}
  "));
          if let Some(uuid) = obj.get("uuid")...
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: Совпадает: parse_ss_uri/parse_vmess_uri кладут cipher/alterId в карту, но вывод перечислял только фиксированный набор полей. Тем же исправлением в вывод попадают все поля парсера (часть бага 79 — транспорты, которые парсер уже извлекает); сам 79 будет проверен отдельно.
- Первопричина одним предложением: Вывод YAML был ручным перечнем полей вместо сериализации всей карты узла.
- Тест: `subscription_parser.rs::tests::regression_bug_76_ss_and_vmess_keep_cipher_and_alter_id`; spec: `be:regression_bug_76`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): Рабочий конвертер на base64-подписке с ss:// и vmess:// → в YAML есть `cipher: aes-256-gcm`, `cipher: chacha20-poly1305`, `alterId: 2`.
- RED: `docs/fix_logs/bug-76-red.log`, red_kind=assert (`SS cipher:` — поля нет в выводе).
- Исправление (что и почему, 1–3 предложения): Узел выводится общим эмиттером `emit_yaml_entry` по всем ключам карты (порядок: type, server, port, остальные по алфавиту), строки экранируются (см. 77); перечисления type/network/cipher без кавычек, как прежде.
- Какие инварианты затрагивает и почему не нарушены: Формат провайдера остался блочным YAML; все прежние тесты конвертера (12) проходят без изменений.
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): SS/VMess-узлы из v2ray-подписок становятся рабочими для failover/БС/гейминга. На роутере с реальной подпиской не проверялось.
- GREEN: `docs/fix_logs/bug-76-green.log`; строка `=== GATE OK bug N | diff ... ===` скопирована из вывода:
  `=== GATE OK bug 76 | diff b940c4dce003 | лог docs/fix_logs/bug-76-green.log ===`
- Статус: FIXED

### Баг 77 — YAML провайдера собирается format! без экранирования
- Правила перечитаны: 2026-11-05 06:02:00 и 06:06:06 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/subscription_parser.rs:605` — `convert_subscription_to_mihomo_yaml`
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
          if let Some(password) = obj.get("password").and_then(|v| v.as_str()) {
              yaml.push_str(&format!("    password: \"{password}\"
  "));
          }
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: Совпадает: server/uuid/password/sni/path/headers подставлялись в кавычки без экранирования, имя экранировало только двойную кавычку, но не обратный слэш и переводы строк — возможна инъекция ключей из подписки. Первый GREEN упал на постороннем нестабильном тесте failover (см. «Новые находки»), повторный — GATE OK.
- Первопричина одним предложением: Отсутствие YAML-экранирования строковых значений.
- Тест: `subscription_parser.rs::tests::regression_bug_77_provider_yaml_scalars_are_escaped`; spec: `be:regression_bug_77`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): Рабочий конвертер на ss:// с паролем, содержащим кавычку и обратный слэш, и именем с переводом строки и `type: direct`: пароль экранирован, ровно одна строка `type:`, имя записано одним экранированным скаляром.
- RED: `docs/fix_logs/bug-77-red.log`, red_kind=assert (`пароль экранирован:`).
- Исправление (что и почему, 1–3 предложения): `yaml_quote` экранирует обратный слэш, кавычку, перевод строки, возврат каретки, табуляцию и управляющие символы; все строковые значения и нестандартные ключи проходят через него.
- Какие инварианты затрагивает и почему не нарушены: Чужая подписка не может внедрить ключи или узлы в конфиг Mihomo.
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Только генерация файлов провайдеров.
- GREEN: `docs/fix_logs/bug-77-green.log`; строка `=== GATE OK bug N | diff ... ===` скопирована из вывода:
  `=== GATE OK bug 77 | diff b940c4dce003 | лог docs/fix_logs/bug-77-green.log ===`
- Статус: FIXED

### Баг 80 — Фоновая «автомиграция» каждый час переводит все http-провайдеры в формат конвертера
- Правила перечитаны: 2026-11-05 06:08:52 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/api.rs:3344` — `sync_all_mobile_providers` (вызывается из main.rs:631/644/650 — старт и каждый час)
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
      for (pid, raw_url) in &provider_urls {
          if !cfg.mobile_provider_urls.contains_key(pid) {
              ...
                          if let Ok(new_y) = crate::routing::convert_http_provider_to_file_in_yaml(&y, pid, &format!("./providers/{pid}.yaml")) {
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: Совпадает: каждый http-провайдер из config.yaml, не помеченный как мобильный, тянулся с UA v2rayNG, конвертировался lossy-парсером и переписывался на type: file. Обновление явно мобильных подписок (mobile_provider_urls) сохранено. Квоты http-провайдеров Mihomo отдаёт сам (subscriptionInfo).
- Первопричина одним предложением: Фоновая задача применяла миграцию формата ко всем провайдерам без согласия пользователя.
- Тест: `api.rs::settings_and_service_regression_tests::regression_bug_80_background_sync_never_migrates_http_providers`; spec: `be:regression_bug_80`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): Структурный тест по исходнику: в теле sync_all_mobile_providers нет convert_http_provider_to_file_in_yaml и обхода всех provider_urls. Ограничение: текстовая проверка; функция сетевая.
- RED: `docs/fix_logs/bug-80-red.log`, red_kind=assert (`фоновая синхронизация не меняет тип провайдеров в config.yaml`).
- Исправление (что и почему, 1–3 предложения): Удалён цикл автомиграции; функция обновляет только mobile_provider_urls (HWID берётся из YAML или из mobile_provider_hwids).
- Какие инварианты затрагивает и почему не нарушены: INV-5: фон не меняет настройки пользователя. Нет лишних reload Mihomo каждый час (заодно снимает часть бага 83).
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Failover/БС продолжают работать с исходными http-подписками.
- GREEN: `docs/fix_logs/bug-80-green.log`; строка `=== GATE OK bug N | diff ... ===` скопирована из вывода:
  `=== GATE OK bug 80 | diff f6784b4f8f68 | лог docs/fix_logs/bug-80-green.log ===`
- Статус: FIXED

### Баг 122 — switch_server переключает на цель все selector-группы, включая 🛡️ Whitelist и группы пользователя
- Правила перечитаны: 2026-11-05 06:11:53 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/mihomo.rs:1225` — `switch_server_inner`
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
      for (name, p) in &proxies {
          let typ = p.get("type").and_then(|t| t.as_str()).unwrap_or("").to_lowercase();
          if typ != "selector" {
              continue;
          }
          // исключаем авто-группы, специализированные AI/Flow группы, игровые группы (RT-01), группы устройств и тестовый Canary Probe
          ...
          groups.push(name.clone());
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: Совпадает: в список попадали все селекторы кроме AI/гейминга/устройств/canary, в т.ч. 🛡️ Whitelist и сервисные группы пользователя; ручное переключение и каждый failover их перетирали. Группу БС failover переключает явно (switch_selector в failover.rs:1677/1763/1819), поэтому её исключение не ломает переходы БС.
- Первопричина одним предложением: Отбор групп по типу, а не по тому, следуют ли они за основным выбором.
- Тест: `mihomo.rs::tests::regression_bug_122_switch_server_keeps_whitelist_and_user_groups`; spec: `be:regression_bug_122`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): `select_groups_to_switch` (её вызывает switch_server_inner): из PROXY / Telegram (на том же узле) / YouTube (на другом) / 🛡️ Whitelist / 🎮 Gaming переключаются только PROXY и Telegram.
- RED: `docs/fix_logs/bug-122-red.log`, red_kind=compile. Ограничение: не доказательство RED — отбор был встроен в сетевую функцию.
- Исправление (что и почему, 1–3 предложения): Вынесена `select_groups_to_switch`: исключает также группу БС; при наличии PROXY/GLOBAL/Proxy прочие группы переключаются только если стоят на том же выборе, что основная, или на прежнем активном узле. Без основной группы — прежнее поведение.
- Какие инварианты затрагивает и почему не нарушены: INV-1: группа БС не уходит на VPS при ручном переключении. INV-7: сброс соединений по-прежнему селективный.
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Сервисные группы пользователя, БС, гейминг, Google AI и группы устройств сохраняют свой выбор.
- GREEN: `docs/fix_logs/bug-122-green.log`; строка `=== GATE OK bug N | diff ... ===` скопирована из вывода:
  `=== GATE OK bug 122 | diff 81ec0c8825c8 | лог docs/fix_logs/bug-122-green.log ===`
- Статус: FIXED

### Баг 39 — Целевой узел БС из подписки ломает конфиг
- Правила перечитаны: 2026-11-05 06:16:17 и 06:22:21 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/routing.rs:2240` — `apply_whitelist_rules` (`dev_target`)
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
          let dev_target = match cfg.whitelist.target_server.as_deref() {
              Some(s) if !s.trim().is_empty() && s.trim() != "follow_failover" => s.trim(),
              _ => WHITELIST_GROUP_NAME,
          };
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: Совпадает: target_server (имя ноды подписки) уходил целью в catch-all SRC-IP-CIDR правила БС. Для БС исправлено полностью: правило всегда на группу 🛡️ Whitelist, а target_server выставляется в группу в рантайме (switch_group в sync_whitelist_mode_tunnel, api.rs:882) — выбор узла сохраняется.
- Первопричина одним предложением: Цели правил Mihomo не проверялись на существование (INV-4).
- Тест: `routing.rs::tests::regression_bug_39_...`; spec: `be:regression_bug_39_`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): Рабочий apply_routing с whitelist.enabled и target_server = нода подписки: ни одно правило не ссылается на ноду; плюс sanitize_rule_targets не меняет пользовательское DOMAIN-REGEX с запятыми (проверено, что без ограничения AUTO-блоками эта часть падает).
- RED: `docs/fix_logs/bug-39-red.log`, red_kind=assert — правило с нодой подписки присутствовало в выводе apply_routing.
- Исправление (что и почему, 1–3 предложения): dev_target всегда WHITELIST_GROUP_NAME. Общая страховка INV-4 в конце apply_routing: `sanitize_rule_targets` проверяет цель каждого правила ВНУТРИ AUTO-блоков (правила пользователя не трогаются) — допустимы группы proxy-groups, статические прокси и DIRECT/REJECT/REJECT-DROP/PASS/COMPATIBLE/GLOBAL; недопустимая цель заменяется группой блока (🛡️ Whitelist в блоке БС, 🎮 Gaming в блоке гейминга, иначе основная прокси-группа) с предупреждением в лог.
- Какие инварианты затрагивает и почему не нарушены: INV-4 соблюдается для всех AUTO-блоков; INV-1: в блоке БС подстановка только 🛡️ Whitelist (не VPS).
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Все 484 теста бэкенда проходят; AUTO-блоки других режимов с валидными целями не меняются.
- GREEN: `docs/fix_logs/bug-39-green.log`; строка `=== GATE OK bug N | diff ... ===` скопирована из вывода:
  `=== GATE OK bug 39 | diff df0422c49ff4 | лог docs/fix_logs/bug-39-green.log ===`
- Статус: FIXED

### Баг 12 — «Выходной узел» игрового устройства из подписки ломает конфиг
- Правила перечитаны: 2026-11-05 06:16:17 и 06:22:21 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/routing.rs:1843` — правила гейминга (`dev.server`)
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
                  let target = dev
                      .server
                      .as_ref()
                      .filter(|s| !s.trim().is_empty())
                      .map(|s| s.as_str())
                      .unwrap_or(GAMING_GROUP_NAME);
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: Подтверждено (BS-PIN: правила не могут ссылаться на ноды подписок). Ограничение: выбранный пользователем узел подписки, если для него нет группы, не учитывается — трафик идёт через группу блока (конфиг валиден, но выбор узла теряется); полноценная поддержка требует per-feature select-групп с use: провайдеров.
- Первопричина одним предложением: Цели правил Mihomo не проверялись на существование (INV-4).
- Тест: `routing.rs::tests::regression_bug_12_...`; spec: `be:regression_bug_12_`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): Рабочий apply_routing с игровым устройством, у которого server = нода подписки: правило с этой целью не появляется (заменено на 🎮 Gaming/основную группу).
- RED: `docs/fix_logs/bug-12-red.log`, red_kind=assert — правило с нодой подписки присутствовало в выводе apply_routing.
- Исправление (что и почему, 1–3 предложения): Общая страховка INV-4 в конце apply_routing: `sanitize_rule_targets` проверяет цель каждого правила ВНУТРИ AUTO-блоков (правила пользователя не трогаются) — допустимы группы proxy-groups, статические прокси и DIRECT/REJECT/REJECT-DROP/PASS/COMPATIBLE/GLOBAL; недопустимая цель заменяется группой блока (🛡️ Whitelist в блоке БС, 🎮 Gaming в блоке гейминга, иначе основная прокси-группа) с предупреждением в лог.
- Какие инварианты затрагивает и почему не нарушены: INV-4 соблюдается для всех AUTO-блоков; INV-1: в блоке БС подстановка только 🛡️ Whitelist (не VPS).
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Все 484 теста бэкенда проходят; AUTO-блоки других режимов с валидными целями не меняются.
- GREEN: `docs/fix_logs/bug-12-green.log`; строка `=== GATE OK bug N | diff ... ===` скопирована из вывода:
  `=== GATE OK bug 12 | diff df0422c49ff4 | лог docs/fix_logs/bug-12-green.log ===`
- Статус: FIXED

### Баг 51 — Узел из подписки для Google AI ломает каждое сохранение
- Правила перечитаны: 2026-11-05 06:16:17 и 06:22:21 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/routing.rs:3276` — `flow_target` / `apply_flow_rules`
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
      let flow_target = cfg.flow_server.as_deref().filter(|s| !s.trim().is_empty()).unwrap_or("PROXY");
      let flow_group = find_flow_group_name(&current);
      if let Ok(with_flow) = apply_flow_rules(&current, flow_target.trim(), flow_group.as_deref()) {
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: Совпадает: без группы Google AI правила Flow получали сырое имя ноды. Ограничение: выбранный пользователем узел подписки, если для него нет группы, не учитывается — трафик идёт через группу блока (конфиг валиден, но выбор узла теряется); полноценная поддержка требует per-feature select-групп с use: провайдеров.
- Первопричина одним предложением: Цели правил Mihomo не проверялись на существование (INV-4).
- Тест: `routing.rs::tests::regression_bug_51_...`; spec: `be:regression_bug_51_`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): Рабочий apply_routing с flow_server = нода подписки и без группы Google AI: правило с этой целью не появляется.
- RED: `docs/fix_logs/bug-51-red.log`, red_kind=assert — правило с нодой подписки присутствовало в выводе apply_routing.
- Исправление (что и почему, 1–3 предложения): Общая страховка INV-4 в конце apply_routing: `sanitize_rule_targets` проверяет цель каждого правила ВНУТРИ AUTO-блоков (правила пользователя не трогаются) — допустимы группы proxy-groups, статические прокси и DIRECT/REJECT/REJECT-DROP/PASS/COMPATIBLE/GLOBAL; недопустимая цель заменяется группой блока (🛡️ Whitelist в блоке БС, 🎮 Gaming в блоке гейминга, иначе основная прокси-группа) с предупреждением в лог.
- Какие инварианты затрагивает и почему не нарушены: INV-4 соблюдается для всех AUTO-блоков; INV-1: в блоке БС подстановка только 🛡️ Whitelist (не VPS).
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Все 484 теста бэкенда проходят; AUTO-блоки других режимов с валидными целями не меняются.
- GREEN: `docs/fix_logs/bug-51-green.log`; строка `=== GATE OK bug N | diff ... ===` скопирована из вывода:
  `=== GATE OK bug 51 | diff df0422c49ff4 | лог docs/fix_logs/bug-51-green.log ===`
- Статус: FIXED

### Баг 64 — Пакетное «Сервер для N устройств» (/api/routing) не сохраняется в device_routing
- Правила перечитаны: 2026-11-05 06:26:16 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/api.rs` — `apply_routing` (POST /api/routing, main.rs:776)
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
      let assignments: Vec<routing::Assignment> = req
          .assignments
          .into_iter()
          .map(|a| routing::Assignment { ip: a.ip, name: a.name, server: a.server })
          .collect();
      ...
              let gname = routing::group_name_for(&a.ip, &a.name);
              if mihomo::switch_group(&state.http, &cfg, &gname, server).await.is_ok() {
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: Совпадает по трём подпунктам: (1) выбор только в YAML/runtime, теряется при пересборке apply_routing (она строит назначения из device_routing) и рестарте Mihomo; (2) UI читает device_routing — назначений не видно; (3) switch_group на каждое устройство — N полных сбросов соединений.
- Первопричина одним предложением: Обработчик не записывал назначения в config.device_routing и использовал переключение со сбросом соединений.
- Тест: `api.rs::settings_and_service_regression_tests::regression_bug_64_batch_routing_persists_to_device_routing`; spec: `be:regression_bug_64`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): `merge_assignments_into_device_routing` (её вызывает apply_routing до коммита): новое устройство получает цепочку из одного сервера, существующее — сервер первым с сохранением остальной цепочки и порога, «default» снимает назначение; структурно — в теле обработчика нет switch_group(.
- RED: `docs/fix_logs/bug-64-red.log`, red_kind=compile — не доказательство RED (функции не было); поведение «до» видно из цитаты.
- Исправление (что и почему, 1–3 предложения): Назначения мержатся в device_routing в той же транзакции; перевыбор узла в группах устройств — switch_selector без сброса соединений LAN.
- Какие инварианты затрагивает и почему не нарушены: INV-3 (одна транзакция), INV-7 (без лишних сбросов).
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Игровые устройства в Compatibility по-прежнему получают server; персональный failover устройства видит новую цепочку.
- GREEN: `docs/fix_logs/bug-64-green.log`; строка `=== GATE OK bug N | diff ... ===` скопирована из вывода:
  `=== GATE OK bug 64 | diff 737daf72067b | лог docs/fix_logs/bug-64-green.log ===`
- Статус: FIXED

### Баг 41 — В режиме БС IPv6 не перехватывается совсем
- Правила перечитаны: 2026-11-05 06:28:40 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/api.rs` — `build_whitelist_full_tunnel_commands` (используется sync_whitelist_full_tunnel и emergency_cleanup_full_tunnel_rules)
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
      // Перехват по приватным подсетям (зеркалирует SRC-IP-CIDR правила БС в Mihomo)
      for subnet in ["192.168.0.0/16", "10.0.0.0/8", "172.16.0.0/12"] {
      ...
      (cleanup_cmd, setup_cmds)
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: Совпадает: в командах БС только iptables (IPv4), ip6tables нет — IPv6 клиентов LAN уходил напрямую, нарушая «строго два канала». Выбран отказ IPv6 вместо IPv6-TPROXY (Mihomo/ядро роутера для v6 TPROXY не проверены). На роутере не проверялось: поведение клиентов (Happy Eyeballs) и наличие модуля mac у ip6tables — NEEDS_ROUTER-риск, исправление само по себе применимо.
- Первопричина одним предложением: Полный перехват БС строился только для IPv4.
- Тест: `api.rs::settings_and_service_regression_tests::regression_bug_41_whitelist_strict_blocks_ipv6_bypass`; spec: `be:regression_bug_41`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): Рабочий build_whitelist_full_tunnel_commands при StrictXKeen: есть цепочка XKEEN_BS6 в FORWARD для br+, RETURN для fe80::/10 и исключённого MAC, финальный REJECT; очистка удаляет переход и цепочку; без БС правил IPv6 нет.
- RED: `docs/fix_logs/bug-41-red.log`, red_kind=assert (`IPv6 из LAN перехватывается цепочкой БС:`).
- Исправление (что и почему, 1–3 предложения): В setup добавлена цепочка ip6tables XKEEN_BS6 (RETURN для link-local/ULA/multicast/loopback и MAC исключённых устройств, REJECT tcp-reset / icmp6 для остального), подключённая в FORWARD для br+/Bridge+; в cleanup — парное удаление.
- Какие инварианты затрагивает и почему не нарушены: INV-1: при БС нет третьего канала через IPv6. INV-6: -w, идемпотентно (-F перед заполнением), есть очистка; emergency cleanup её использует.
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): Только при StrictXKeen; без БС и в других стратегиях IPv6 не ограничивается. Гейминг взаимно исключается с БС в sync_full_tunnel_modes.
- GREEN: `docs/fix_logs/bug-41-green.log`; строка `=== GATE OK bug N | diff ... ===` скопирована из вывода:
  `=== GATE OK bug 41 | diff fa069d251495 | лог docs/fix_logs/bug-41-green.log ===`
- Статус: FIXED

### Баг 45 — DNS-порт Mihomo 1053 зашит примерно в 100 местах
- Правила перечитаны: 2026-11-05 06:31:22 и 06:33:25 | хеш bc29a182fac8
- Где код (найдено grep по имени функции): `backend/src/api.rs:10444` — `resolve_active_dns_port_with_health` (источник DNS-порта для туннелей БС/гейминга); остальные вхождения: api.rs (~71, в т.ч. скрипт S51zapret и тесты), adguard.rs (~30, NDM-хуки и sync-скрипты «1053|5353|5354»), zapret.rs (~14), watchdog.rs (6)
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```rust
      if (app_cfg.adguard.enabled || app_cfg.adblock_enabled) && adguard_is_alive {
          crate::adguard::detect_adguard_dns_port()...
      } else {
          1053
      }
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки: Подпункты: 45a — порт для DNS-редиректа БС и гейминга (resolve_active_dns_port) — FIXED (`=== GATE OK bug 45a | diff 6f66d490fc0b | лог docs/fix_logs/bug-45a-green.log ===`, red_kind=assert: `left: 1053`). 45b — шаблоны NDM-хуков и sync-скрипты AdGuard (`1053|5353|5354`), 45c — S51zapret и DNS_REDIRECT_REMOVE_CMD (зависят от решения по багу 31), 45d — проверки сторожа (`to-ports 1053`, check_dns_resolver_ready(1053)) — НЕ исправлены.
- Первопричина одним предложением: Отсутствие единого источника DNS-порта Mihomo.
- Тест: `api.rs::settings_and_service_regression_tests::regression_bug_45a_mihomo_dns_port_read_from_dns_listen` (только 45a); spec: `be:regression_bug_45a`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт): Рабочий resolve_active_dns_port_with_health при выключенном AdGuard читает dns.listen из config.yaml (7853) и падает на 1053 только без listen.
- RED: 45a: `docs/fix_logs/bug-45a-red.log`, red_kind=assert. 45b–45d: не запускались.
- Исправление (что и почему, 1–3 предложения): 45a: добавлены `routing::parse_mihomo_dns_listen_port` и константа DEFAULT_MIHOMO_DNS_PORT; resolve_active_dns_port берёт порт из config.yaml Mihomo.
- Какие инварианты затрагивает и почему не нарушены: INV-9 восстановлен для DNS-редиректа БС/гейминга; для скриптов AdGuard/Zapret — нет.
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): БС и гейминг редиректят DNS на реальный порт Mihomo.
- GREEN: не запускался — BLOCKED
- Статус: BLOCKED (Не продуктовый вопрос, а объём: оставшиеся ~100 мест (шаблоны shell-скриптов NDM/AdGuard/Zapret) требуют параметризации шаблонов и проверки на роутере; S51zapret зависит от решения по багу 31. Следующий шаг: передать порт в build_sync_tunnel_dns_ports_script/build_adguard_ndm_hook_content и zapret.conf (DNS_PORT).)

<!-- Шаблон. Копировать целиком для каждого бага, заполнять ВСЕ поля.

### Баг N — <короткое название из BUG_AUDIT.md>
- Правила перечитаны: <время и хеш из вывода `fix_gate.sh rules`>
- Где код (найдено grep по имени функции): `<файл>:<строка>` — `<имя функции>`
- Цитата старого кода (3–15 строк, скопировано из вывода sed, не по памяти):
  ```
  ...
  ```
- Сценарий и подпункты аудита; доказательства совпадения или несовпадения, ограничения проверки:
- Первопричина одним предложением:
- Тест: `<путь к тесту>::<имя>`; spec: `be:<фильтр>` или `fe:<файл>::<имя>`
- Что тест проверяет (какую строку РАБОЧЕГО кода вызывает и какое поведение ждёт):
- RED: `docs/fix_logs/bug-N-red.log`, реальная причина падения и строка из лога; либо ALREADY с diff и объяснением отсутствия RED:
- Исправление (что и почему, 1–3 предложения):
- Какие инварианты затрагивает и почему не нарушены:
- Не задеты ли соседние режимы (БС / гейминг / Zapret / AdGuard / failover / Google AI): 
- GREEN: `docs/fix_logs/bug-N-green.log`; строка `=== GATE OK bug N | diff ... ===` скопирована из вывода:
- Статус: FIXED / NOT_REPRODUCED / NEEDS_ROUTER / BLOCKED (+ вопрос пользователю)
-->

## Поправки после самопроверки (2026-11-05 04:55)

- **Баг 72 — запись выше была неверной, статус FIXED тогда не подтверждался.** `split_connections_delta` вызывалась только из теста, в рабочем `run_mihomo_stream` весь `/traffic` по-прежнему шёл в VPS/БС. Кроме того, тест читал глобальный `WHITELIST_ACTIVE` и нестабильно падал при параллельных тестах. Исправлено вместе с 73: подсчёт перенесён в `run_connections_accounting`, тест переведён на явный параметр `whitelist_active`. Повторный GREEN: `=== GATE OK bug 72 | diff df3e87c878e0 | лог docs/fix_logs/bug-72-green.log ===`.
- **Баг 93 — исправление вносило ABBA-взаимоблокировку.** `init_revoked_tokens_storage` держал `REVOKED_TOKENS_PATH` и брал `REVOKED_TOKENS`, `revoke_token` — наоборот (через `persist_revoked_tokens`). Тестовый прогон зависал больше 10 минут. Лок пути теперь отпускается до загрузки; тесты отзыва сериализованы `REVOKED_TEST_LOCK` (сами проверки не ослаблялись). Повторный GREEN: `=== GATE OK bug 93 | diff df3e87c878e0 | лог docs/fix_logs/bug-93-green.log ===`. Полный `cargo test` прогнан 4 раза подряд — 456/456.
- **Предупреждения `never used` в сборке под Windows** у `build_gaming_full_tunnel_commands` и др. — артефакт `#[cfg(target_os = "linux")]`; на Linux функции вызываются (`sync_gaming_full_tunnel`), тесты бага 70 проверяют рабочий код.
- Логи `bug-selftest*-red.log` (проверка самого шлюза до init) удалены перед коммитом — к багам не относились.

## Новые находки

- `backend/src/notifications.rs` (тест `validate_telegram_bot_token`) содержит строку, похожую на настоящий токен бота `8959891087:AAH…`. Если токен реальный — его нужно отозвать у @BotFather и заменить в тесте на фиктивный.
- `backend/src/failover.rs` — `test_adversarial_quarantine_12h_amnesty_boundary_oracle` нестабилен по времени: берёт `now`, а функция загрузки сравнивает с собственным `now`; при переходе секунды граничная запись (ровно 43200 с) становится 43201 с и удаляется. Упал один раз в GREEN бага 77 (повторный прогон зелёный). Исправить можно, передав время в функцию загрузки явно — отдельной задачей.

## Итог сессии 2026-11-05 (релиз v1.10.1)

- **FIXED: 46** — 1, 2, 3, 12, 27, 28, 39, 41, 44, 51, 56, 57, 59, 61, 62, 64, 68, 70, 71, 72, 73, 76, 77, 80, 84, 88, 89, 90, 91, 92, 93, 94, 97, 102, 103, 109, 114, 116, 118, 122, 125, 128, 130, 133, 135, 141.
- **BLOCKED: 4** — 31, 38, 45, 69 (у 31, 38, 45 однозначные подпункты 31a, 38b, 45a исправлены и прошли шлюз; остальное ждёт решения пользователя или отдельной работы; вопросы — в записях).
- **NOT_REPRODUCED / NEEDS_ROUTER: 0.**
- **Не начаты: 91** — 4, 5, 6, 7, 8, 9, 10, 11, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 29, 30, 32, 33, 34, 35, 36, 37, 40, 42, 43, 46, 47, 48, 49, 50, 52, 53, 54, 55, 58, 60, 63, 65, 66, 67, 74, 75, 78, 79, 81, 82, 83, 85, 86, 87, 95, 96, 98, 99, 100, 101, 104, 105, 106, 107, 108, 110, 111, 112, 113, 115, 117, 119, 120, 121, 123, 124, 126, 127, 129, 131, 132, 134, 136, 137, 138, 139, 140. Это фазы C (некритичные), D (конфликты режимов), E (Google AI), F, G (матрица кнопок, п.50) и H (интерфейс, пп. 47–49) — они не вошли в этот релиз.
- Проверки перед коммитом: полный `cargo test` (Windows) — все тесты зелёные; `cargo check --target aarch64-unknown-linux-musl --tests` — компилируется; `npx vitest run`, `npx tsc --noEmit`, `npm run build` — зелёные. Запуск тестов под Linux выполняет только CI.
- Ограничения: ничего не проверялось на реальном роутере до деплоя; red_kind=compile у части пунктов не доказывает воспроизведение (отмечено в записях); шаг backend-тестов в CI идёт с `|| true` и сам по себе не гарантирует зелёный Rust.
