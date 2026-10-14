# 🚀 МАСТЕР-ПРОМПТ ДЛЯ УСТРАНЕНИЯ ДЕФЕКТОВ И РЕФАКТОРИНГА XKEEN ROUTE

Скопируйте текст ниже и вставьте его в новый чат с ИИ-ассистентом:

```markdown
# 🛠️ РОЛЬ И КОНТЕКСТ
Ты — Senior Rust Architect, Principal Embedded Systems Engineer и Security Auditor.
Мы работаем над кодовой базой **XKeen Route** (веб-панель и управляющий демон маршрутизации для роутеров Keenetic на базе Entware/Linux/BusyBox, MIPS/ARM).

### 📚 Стек проекта:
- **Бэкенд**: Rust 2021, Axum 0.8, Tokio, Reqwest, Serde, Sysinfo. Взаимодействует с ядром Mihomo (Clash core), Zapret (DPI bypass), AdGuard Home, RCI (HTTP API KeeneticOS), iptables/ipset/ip rule.
- **Фронтенд**: React 18, TypeScript, Vite, Tailwind CSS SPA.
- **Скрипты**: Shell/Bash (`setup.sh`), NDM netfilter-хуки.

---

## 📌 ВВОДНЫЕ ДАННЫЕ И РЕЗУЛЬТАТЫ АУДИТА
В проекте был проведён 100% построчный сквозной аудит всех 88 файлов кодовой базы.
Полный реестр из **61 подтверждённого дефекта** с детальным описанием первопричин, затронутых строк и готовыми рекомендациями зафиксирован в файле:
👉 `docs/MASTER_AUDIT_REGISTRY.md` (а также детализированы в `docs/external_audit_part1_findings.md` .. `part5_findings.md`).

---

## ⚠️ ОБЯЗАТЕЛЬНЫЕ ПРАВИЛА И ИНВАРИАНТЫ РАЗРАБОТКИ
1. **Анти-регрессионные тесты**:
   На КАЖДЫЙ исправляемый баг, уязвимость или хардкод ты ОБЯЗАН написать автоматический тест (Rust unit/integration test в `backend/src/...` или Vitest в `frontend/src/...`), который проверяет этот сценарий и гарантирует, что дефект больше никогда не повторится. Все тесты должны успешно проходить.
2. **Иерархия блокировок (Lock Ordering)**:
   При любых транзакциях и мутациях конфигов строгий порядок захвата мьютексов:
   `state.config_lock -> state.routing_lock`.
   Никаких прямых записей в `config.yaml` или `config.json` в обход локов или транзакции `ConfigTx`!
3. **Безопасность шелла и CLI (Zero Command Injection)**:
   Категорически запрещена интерполяция пользовательского ввода в `sh -c`, `iptables`, `ndmc -c`. Все IP, MAC, домены, ID провайдеров и пути обязаны проходить строгую валидацию по белым спискам (`is_valid_domain`, `is_valid_ip_or_cidr`, `is_valid_mac`, `is_valid_provider_id`, проверка допустимости пути в `/opt/etc/init.d/`).
4. **Защита конфигурации от стирания (No Silent Defaults Reset)**:
   Ошибки разбора JSON/YAML никогда не должны молча откатывать систему к `AppConfig::default()`, стирать хэши паролей администратора или отключать авторизацию.
5. **Синхронизация контрактов API (Serde vs TypeScript)**:
   Поля структур обязаны строго совпадать (например, `target`/`host` в `RecentGamingConn`, `s.id` вместо `s.name` в расписаниях).

---

## 🎯 ПЛАН УСТРАНЕНИЯ ДЕФЕКТОВ (ПО ЭТАПАМ)

### ЭТАП 1: КРИТИЧЕСКИЙ БЛОК P0 (Устранить немедленно)
1. **[BUG-21]** `setup.sh` vs `updater.rs`: Унифицировать имена MIPS-артефактов (`mips32le`/`mipsel` и `mips32`/`mips`), чтобы восстановить установку и автообновление на MIPS-роутерах Keenetic.
2. **[BUG-16]** `watchdog.rs`: Устранить бесконечный 4-секундный цикл перезаписи `config.yaml` и релоада Mihomo, когда все `force_domains` перехвачены Zapret Direct (`missing_force = true`).
3. **[BUG-15]** `mihomo.rs` & `watchdog.rs`: Перевести `switch_flow_server` на `ConfigTx`/локи, а в `watchdog.rs` захватывать `config_lock` перед `routing_lock`.
4. **[FE-01]** `Devices.tsx` & `DeviceScheduleModal.tsx`: Передавать `s.id` вместо `s.name` в расписания устройств, починив переключение нод с эмодзи/флагами.
5. **[SEC-01]** `api.rs` (`sync_zapret_files`): Валидировать `excluded_devices` (IP/CIDR и MAC) перед записью в `zapret.conf` и вызовом `iptables`.
6. **[SEC-02]** `antigravity.rs`: Проверять домены через `is_valid_domain` перед вызовом `ndmc -c "ip host {target} {ip}"`.
7. **[SEC-03]** `api.rs`: Закрыть Path Traversal в `delete_provider`, `add_provider` и `import_node` через строгий валидатор `is_valid_provider_id`.
8. **[BUG-01]** `antigravity.rs`: Исправить парсер DNS-ответов со сжатыми указателями `0xC0` и дедуплицировать удаление старых правил `ip rule`.

### ЭТАП 2: БЛОК ВЫСОКОГО ПРИОРИТЕТА P1
1. **Безопасность**:
   - `SEC-04` (`antigravity.rs`): Ограничить CONNECT-прокси только доменами из `targets` на порт 443, закрыть доступ к `127.0.0.1` и LAN.
   - `SEC-05` (`api.rs`/`adguard.rs`): Запретить кавычки `"` и `\n` в `init_script`.
   - `SEC-06` (`adguard.rs`): Убрать `-k` / `--no-check-certificate` при скачивании AdGuard Home.
   - `SEC-07` (`api.rs`): Маскировать `adguard.password` в `get_settings`, игнорировать `"******"` при сохранении.
   - `SEC-08` (`auth.rs`): Убрать слепое доверие к `*.keenetic.pro` в CSRF Guard, требовать валидный заголовок.
   - `SEC-09` (`api.rs`): Валидировать `dev.ip` в `build_gaming_full_tunnel_commands`.
   - `SEC-12` (`routing.rs`): Экранировать кавычки и `\n` в `add_provider_to_yaml_full` и `apply_flow_rules`.
   - `SEC-13` (`updater.rs`): Ограничить `custom_url` в `mihomo_install` только официальным репозиторием GitHub.
2. **Ядро, Маршрутизация и Синхронизация**:
   - `BUG-02` (`antigravity.rs`): Сделать перезапуск литенера при смене порта через канал отмены.
   - `BUG-03` (`api.rs`): Обернуть `add_provider`, `delete_provider`, `set_ignore`, `fix_names` в `ConfigTx`.
   - `BUG-04` (`api.rs`): Исправить конкатенацию `proxies:` при однострочном `proxies: []`.
   - `BUG-10` (`api.rs`/`main.rs`): Добавить хук восстановления правил `xkeen-gaming-compat` при перестройке netfilter.
   - `BUG-11` (`config.rs`): Извлекать хэш пароля регуляркой из повреждённого JSON и блокировать автоперезапись дефолтами.
   - `BUG-17` (`routing.rs`): Динамически определять имя прокси-группы через `find_proxy_target_group` в `apply_domain_rules`.
   - `BUG-23` (`watchdog.rs`): Исключить порты STUN/WebRTC из Smart Auto-Gaming и не сбрасывать режим `BypassRu`.
   - `BUG-24` (`watchdog.rs`/`types.ts`): Синхронизировать `target`/`host` в `RecentGamingConn`.
   - `BUG-25` (`watchdog.rs`): Вызывать `sync_gaming_full_tunnel` при смене DHCP IP в `spawn_dhcp_device_monitor`.
3. **Фронтенд и Хардкод**:
   - `FE-02` (`Settings.tsx`): Передавать `expected_revision` и синхронизировать стейт оповещений в общем `save()`.
   - `FE-09` (`nodeParser.ts`): Поддержать сериализацию массивов `alpn: [...]` в YAML.
   - `HC-01`, `HC-05`, `HC-06`: Устранить хардкод IP `192.168.1.1`, прокси `127.0.0.1:7890` и подсети `192.168.0.0/16`.

### ЭТАП 3: СРЕДНИЙ И МИНОРНЫЙ БЛОКИ (P2 / P3)
- По порядку из `docs/MASTER_AUDIT_REGISTRY.md` (включая LIFO порядок правил в `BUG-18`, фоновые запросы `FE-07`, поиск в `ConfigEditor` `FE-06`, и оставшийся хардкод `HC-02..04,07..09`).

---

## 🚦 ФОРМАТ РАБОТЫ
1. Начинаем с **ЭТАПА 1 (Критический блок P0: задачи 1–8)**.
2. Для каждого исправления:
   - Приведи точный diff изменений.
   - Напиши анти-регрессионный тест.
   - Запусти тестовый набор и убедись, что все тесты проходят без ошибок.
3. Дождись подтверждения перед переходом к следующему этапу.

Подтверди готовность и начни с выполнения **Этапа 1 (задачи BUG-21, BUG-16, BUG-15, FE-01, SEC-01..03, BUG-01)**.
```
