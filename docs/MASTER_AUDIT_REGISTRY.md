# 🏛️ Сводный реестр аудита кодовой базы XKeen Route (Master Audit Registry)

> **Статус**: 100% файлов репозитория полностью проанализировано (88 файлов, Parts 1–5).  
> **Всего дефектов**: 61 (14 Security, 26 Bugs/Architecture, 12 Frontend/Contracts, 9 Hardcodes).

---

## 📊 Матрица дефектов по критичности

| Критичность | Безопасность (SEC) | Баги и ядро (BUG) | Фронтенд (FE) | Хардкод (HC) | Всего |
|:---|:---:|:---:|:---:|:---:|:---:|
| **P0 - Critical** | 4 | 3 | 1 | 0 | **8** |
| **P1 - High** | 6 | 9 | 4 | 3 | **22** |
| **P2 - Medium** | 4 | 11 | 4 | 2 | **21** |
| **P3 - Minor** | 0 | 3 | 3 | 4 | **10** |
| **ИТОГО** | **14** | **26** | **12** | **9** | **61** |

---

## 🔴 P0 — КРИТИЧЕСКИЕ ДЕФЕКТЫ (Устранить в первую очередь)

| ID | Область | Файл и функция | Описание дефекта | Последствия |
|:---|:---|:---|:---|:---|
| **SEC-01** | Security | `api.rs` (`sync_zapret_files`) | Отсутствие строгой валидации `excluded_devices` перед записью в `zapret.conf` и передачей в `iptables` | RCE (Command / Argument Injection) от имени root |
| **SEC-02** | Security | `antigravity.rs` (`apply_keenetic_rules`) | Прямая интерполяция массива `targets` в команду `ndmc -c "ip host {target} {ip}"` | Инъекция команд CLI KeeneticOS в обход RCI |
| **SEC-03** | Security | `api.rs` (`delete_provider`, `import_node`) | Невалидированные `req.id` и `provider_name` с поддержкой `..` в путях удаления/записи `.yaml`/`.json` | Path Traversal: удаление `config.json` со сбросом auth |
| **BUG-01** | Routing / Bug | `antigravity.rs` (`parse_dns_a_records`, `apply_keenetic_rules`) | Ошибка парсинга сжатых меток DNS (0xC0 как длина 192) и накопление дубликатов `ip rule` в ядре | Засорение таблицы RPDB и потеря A-записей |
| **BUG-15** | Concurrency | `mihomo.rs` (`switch_flow_server`) & `watchdog.rs` | Запись `config.yaml` без мьютексов и захват `routing_lock` без `config_lock` в вочдоге | Нарушение иерархии блокировок, потеря правил |
| **BUG-16** | Watchdog | `watchdog.rs` (`spawn`) | `missing_force = true` при перехвате всех доменов Zapret Direct вызывает цикл перезаписи каждые 4с | Непрерывная перезапись `config.yaml`, рестарт ядра и износ флеш-памяти |
| **BUG-21** | Installer | `setup.sh` vs `updater.rs` | Расхождение имён бинарников MIPS (`mips32le`/`mips32` в скрипте против `mipsel`/`mips` в коде) | Полный отказ установки или обновления на MIPS Keenetic |
| **FE-01** | Frontend | `Devices.tsx` & `DeviceScheduleModal.tsx` | Передача очищенного `s.name` вместо `s.id` в `target_server` расписания устройств | Расписание падает с HTTP 400 (`proxy not found`) для нод с эмодзи |

---

## 🟠 P1 — ВЫСОКИЙ ПРИОРИТЕТ (Нарушение сети, потеря данных, обход защиты)

| ID | Область | Файл и функция | Описание дефекта |
|:---|:---|:---|:---|
| **SEC-04** | Security | `antigravity.rs` (`handle_proxy_conn`) | SSRF и открытый CONNECT-прокси для любых внешних доменов и локальных портов `127.0.0.1` |
| **SEC-05** | Security | `api.rs` (`xkeen_service`) & `adguard.rs` | Пропуск кавычек `"` и `\n` в валидации `init_script` перед выполнением в `sh -c` |
| **SEC-06** | Security | `adguard.rs` (`install_adguard_package`) | Скачивание бинарника AdGuard с отключенной проверкой TLS (`curl -k`) и без SHA256 |
| **SEC-07** | Security | `api.rs` (`get_settings`, `put_settings`) | Незамаскированный `adguard.password` и риск перезаписи паролей строкой `"******"` |
| **SEC-08** | Security | `auth.rs` (`auth_middleware`, `is_trusted_origin`) | Обход CSRF-проверки при отсутствии Origin/Referer и слепое доверие ко всем `*.keenetic.pro` |
| **SEC-09** | Security | `api.rs` (`build_gaming_full_tunnel_commands`) | Инъекция команд в `iptables`/`sh -c` через невалидированный `dev.ip` в `POST /api/gaming/save` |
| **SEC-12** | Security | `routing.rs` (`add_provider_to_yaml_full`, `apply_flow_rules`) | YAML Injection через неэкранированные кавычки и `\n` в `url`, `hwid`, `user_agent`, `target` |
| **SEC-13** | Security | `updater.rs` (`mihomo_install`) | Скачивание и запуск ELF-бинарника от имени `root` с произвольного адреса `custom_url` |
| **BUG-02** | Concurrency | `antigravity.rs` (`SHUTDOWN`, `start_proxy`) | Одноразовый `AtomicBool` блокирует перезапуск прокси при смене порта в настройках |
| **BUG-03** | Concurrency | `api.rs` (`add_provider`, `delete_provider`) | Запись `config.yaml` в обход транзакции `ConfigTx` и блокировки `routing_lock` |
| **BUG-04** | Core | `api.rs` (`import_node`) | Создание дублирующегося ключа `proxies:` при однострочном `proxies: []` в YAML |
| **BUG-09** | Diagnostics | `api.rs` (`test_dpi`, `run_mini_blockcheck`) | Фиктивные результаты замера прямого канала Zapret (`204`/`45ms`) и эмуляция баллов блочека |
| **BUG-10** | State Desync | `api.rs` (`build_gaming_full_tunnel_commands`) | Сброс правил `xkeen-gaming-compat` при перестройке netfilter роутером (нет NDM-хука) |
| **BUG-11** | Auth / Data Loss | `config.rs` (`parse_config_content`, `load`) | Падение парсера оставляет `auth.enabled=true` с пустым хэшем пароля (вечный deadlock входа) |
| **BUG-17** | Routing | `routing.rs` (`apply_domain_rules`) | Хардкод целевой группы `PROXY` ломает релоад Mihomo на конфигах с группами `Proxy`/`GLOBAL` |
| **BUG-23** | Watchdog | `watchdog.rs` (`spawn_smart_gaming_monitor`) | Любой UDP-пакет >= 1024 (STUN, звонки, торренты) включает игровой режим и сбрасывает `BypassRu` |
| **BUG-24** | Desync | `watchdog.rs` vs `types.ts` (`RecentGamingConn`) | Поле называется `target` в Rust и `host` в TS: в UI пропали домены и сломался игнор |
| **BUG-25** | State Desync | `watchdog.rs` (`spawn_dhcp_device_monitor`) | При смене IP устройства по DHCP не вызывается `sync_gaming_full_tunnel` для iptables |
| **FE-02** | Frontend | `Settings.tsx` (`save`, `saveDomains`) | Потеря `expected_revision` и затирание актуальных настроек Telegram при общем `save()` |
| **FE-09** | Frontend | `nodeParser.ts` (`objectToYamlNode`) | Массив `alpn` сериализуется как строка `alpn: "h3"`, ломая парсинг YAML в Mihomo |
| **HC-01** | Hardcode | `adguard.rs` & `antigravity.rs` | Хардкод fallback IP `192.168.1.1`, DNS `8.8.8.8` и сторонних SmartDNS серверов |
| **HC-05** | Hardcode | `api.rs` (`zapret_action`) | Хардкод `-x http://127.0.0.1:7890` в shell-скриптах вместо `cfg.mihomo_proxy_url()` |
| **HC-06** | Hardcode | `api.rs` (`build_gaming_full_tunnel_commands`) | Хардкод только подсети `192.168.0.0/16` ломает локальный трафик в сетях 10.x и 172.16.x |

---

## 🟡 P2 — СРЕДНИЙ ПРИОРИТЕТ (Функциональные дефекты, производительность, надежность)

- **SEC-10**: SSRF через параметр `url_override` в `do_sync_community_hostlist`.
- **SEC-11**: Переполнение `REVOKED_TOKENS` (> 10 000) через спам в `/api/auth/logout` сбрасывает отозванные сессии.
- **SEC-14**: Отсутствие экранирования HTML в Telegram-алертах ломает отправку при символах `<`/`>` в названии сервера.
- **BUG-05**: Блокирующие синхронные `std::process::Command` и `logger::tail` в воркерах Tokio без `spawn_blocking`.
- **BUG-06**: Несовпадение синтаксиса роутов Axum 0.8 (`:param` vs `{param}`) и 2 МБ лимит на импорт бэкапов.
- **BUG-07**: Деструктивный side-effect в `GET /api/zapret/status` (убийство nfqws и вызов stop-fw в GET-запросе).
- **BUG-08**: Отсутствие `AdGuardHome.yaml` в архивах `.xkbak` при экспорте и блокировка при импорте.
- **BUG-12**: Запись `config.json` в `get_zapret_analytics` без мьютекса `config_lock` (TOCTOU гонка).
- **BUG-13**: Ложное срабатывание детектора IP-групп на внешние правила `DST-IP-CIDR` из-за `payload.contains('/')`.
- **BUG-14**: Отказ сохранения настроек AdGuard Home при остановленном Mihomo из-за `commit_and_reload()`.
- **BUG-18**: Инвертированный LIFO-порядок правил в `apply_routing`: правила устройств перекрывают AdBlock и Direct.
- **BUG-19**: Локальные модификации `provider_filters` уничтожаются при выходе из `apply_routing`.
- **BUG-26**: Неиспользуемый `ZapretLockGuard` в `api.rs` и рассинхронизация флага `SMART_TV_MODE=0`.
- **FE-03**: Несовпадение схемы ответа `notifications/test` (`results` vs `message`), ложный статус успеха в UI.
- **FE-04**: Кнопка `SIGKILL (-9)` и чекбоксы `smart_split`/`fix_nat_fake_ip` игнорируются бэкендом.
- **FE-05**: Битая ссылка скачивания расширения в `FlowRepairModal` (`/flow-unlock-extension.zip`).
- **FE-06**: Поле поиска (`Ctrl+F`) в `ConfigEditor.tsx` не привязано к логике поиска.
- **FE-07**: Сетевые запросы к GitHub в `useEffect` при закрытых модалках `MihomoCoreModal` и `UpdateModal`.
- **FE-11**: Ложные ошибки `yamlValidator.ts` на протоколах `tls://`, `quic://` и IPv6.
- **HC-02**: Автосмена порта AdGuard с 53 на 5353 не обновляет `AppConfig`, хардкод `qnum` и `fwmark`.
- **HC-03**: Оператор `||` между `-i br+` и `-i Bridge+` в `S51ZAPRET_SCRIPT` никогда не доходит до `Bridge+`.

---

## 🟢 P3 — МИНОРНЫЕ ДЕФЕКТЫ И ХАРДКОД

- **BUG-20**: Расчет CPU% в `rci::get_proc_stats` без деления на число ядер и `nfqws_cpu_pct = 0.0`.
- **BUG-22**: Аргумент `beta` в `setup.sh` не влияет на выборку релиза.
- **FE-08**: Пропуск `trafficMap` в зависимостях `useCallback` в `Devices.tsx` и утечка таймера в `UpdateModal`.
- **FE-10**: Тихая обрезка QR-кода при длине URI > 858 байт в `qrCode.ts`.
- **FE-12**: Отсутствие `document.hidden` в таймере `ConnectionsViewer.tsx` и пересоздание слушателя клавиш в `App.tsx`.
- **HC-04**: Захардкоженные дефолтные версии Zapret `v72.13`/`v1.0.5.2`, поиск локаций серверов США по подстрокам.
- **HC-07**: Захардкоженные интерфейсы `apcli1`/`nwg0`, пути в `create_init`.
- **HC-08**: Хардкод `/opt/sbin/mihomo`, DNS `77.88.8.8`, WAN-интерфейса `apcli1`, User-Agent подписок.
- **HC-09**: Хардкод портов `7890`, `1001`, `9090`, устаревшее описание сессии `30 дней` в `Settings.tsx`.
