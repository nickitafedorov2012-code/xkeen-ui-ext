# Результаты аудита Part 5 (Кодовая база XKeen Route)
> Зафиксировано из внешнего анализа нейросети для финальной части репозитория (хвост watchdog.rs, zapret.rs, setup.sh, App.tsx, api.ts, types.ts, ConnectionsViewer.tsx, nodeParser.ts, qrCode.ts, yamlValidator.ts, clipboard.ts, presets.ts).

## 🐛 Раздел 1: Критические баги бэкенда и инсталлятора (setup.sh, watchdog.rs, zapret.rs)

### [BUG-21] Отказ установки на MIPS/MIPSEL из-за несовпадения имён бинарников
- **Category**: Bug / Installer Failure
- **Severity**: P0 - Critical
- **File & Location**: `setup.sh` (строки 31–46) vs `backend/src/updater.rs` (строки 228–234)
- **Проблема**: В `setup.sh` скрипт ищет файлы `xkeen-route-mips32le` и `xkeen-route-mips32`, а в `updater.rs` прописаны `xkeen-route-mipsel` и `xkeen-route-mips`. В зависимости от именования в релизе GitHub, либо `setup.sh` падает с ошибкой определения ссылки загрузки на всех MIPS-роутерах Keenetic, либо встроенное обновление в UI никогда не находит бинарник.
- **Решение**: Поддержать оба варианта (`mips32le`/`mipsel` и `mips32`/`mips`) в `setup.sh` и `updater.rs`.

### [BUG-22] Игнорирование флага beta в setup.sh
- **Category**: Bug
- **Severity**: P3 - Minor
- **File & Location**: `setup.sh` (строки 19–22, 41–46)
- **Проблема**: Переменная `$BETA` парсится из аргументов, но функция `download_url()` всегда запрашивает `/releases` и берет первый попавшийся релиз, скачивая pre-release даже при обычной стабильной установке.
- **Решение**: При `BETA=false` использовать `/releases/latest`, а `/releases` — только при `BETA=true`.

### [BUG-23] Ложные срабатывания Smart Auto-Gaming на фоновый UDP-трафик
- **Category**: Bug / Logical Defect
- **Severity**: P1 - High
- **File & Location**: `backend/src/watchdog.rs` (`is_gaming_connection_match`, `spawn_smart_gaming_monitor`)
- **Проблема**: Любой UDP-пакет на порт >= 1024 (кроме 7 базовых портов) считается игровым подключением. Торренты, WebRTC/STUN звонки в Telegram/WhatsApp, системные службы Windows/macOS мгновенно активируют игровой режим. Кроме того, монитор насильно сбрасывает режим `BypassRu` на `Compatibility`, заворачивая даже российский трафик через VPS.
- **Решение**: Исключить порты STUN/WebRTC (3478, 5349, 19302-19309), не перезаписывать режим `BypassRu` на `Compatibility`.

### [BUG-24] Рассинхронизация полей RecentGamingConn (target в Rust vs host в TypeScript)
- **Category**: Frontend API Desync / Bug
- **Severity**: P1 - High
- **File & Location**: `backend/src/watchdog.rs` (`RecentGamingConn`) vs `frontend/src/types.ts` и `Gaming.tsx`
- **Проблема**: На бэкенде поле называется `target`, а фронтенд ожидает `host`. В итоге в таблице недавней активности вместо доменных имен всегда показывается сырой `ip:port`, а кнопка «Не реагировать» не может добавить домен в игнор-лист.
- **Решение**: Добавить `#[serde(alias = "host")]` или дублировать поле в структуре на бэкенде.

### [BUG-25] Пропуск обновления iptables при смене IP устройства по DHCP в Compatibility Mode
- **Category**: State Desync / Bug
- **Severity**: P1 - High
- **File & Location**: `backend/src/watchdog.rs` (`spawn_dhcp_device_monitor`)
- **Проблема**: При смене IP устройства обновляется только `config.yaml`, но вызов `sync_gaming_full_tunnel` забыт. Правила `iptables` остаются привязаны к старому IP, ломая перехват трафика консоли.
- **Решение**: Вызывать `sync_gaming_full_tunnel` внутри блока `if ip_changed`.

### [BUG-26] Неиспользуемый ZapretLockGuard и рассинхронизация парсинга zapret.conf
- **Category**: Bug / Concurrency
- **Severity**: P2 - Medium
- **File & Location**: `backend/src/zapret.rs` vs `backend/src/api.rs` (`zapret_action`)
- **Проблема**: `ZapretLockGuard::acquire` не вызывается в `zapret_action`, из-за чего статус операции всегда `Idle`, а `watchdog` может параллельно запустить `S51zapret start`. В `save_config` флаг `SMART_TV_MODE="0"` не сбрасывается в `false` из-за ручного дублирования парсинга.
- **Решение**: Использовать `ZapretLockGuard` в `zapret_action` и вызывать `zapret::parse_zapret_conf`.

---

## ⚡ Раздел 2: Уязвимости и баги утилит фронтенда

### [FE-09] Повреждение alpn и ошибки парсинга в nodeParser.ts
- **Category**: Frontend Defect / Bug
- **Severity**: P1 - High
- **File & Location**: `frontend/src/utils/nodeParser.ts` (`objectToYamlNode`, `parseShadowsocks`)
- **Проблема**: Массив `alpn: ["h3"]` в `objectToYamlNode` сериализуется как строка `alpn: "h3"`, вызывая ошибку парсинга YAML в Mihomo. В `parseShadowsocks` параметры плагинов (`?plugin=...`) не отсекаются от `hostport`.
- **Решение**: Добавить поддержку `Array.isArray(val)` в `objectToYamlNode` с генерацией YAML-массива.

### [FE-10] Обрезка данных QR-кода при длине свыше 858 байт
- **Category**: Bug / Frontend Defect
- **Severity**: P3 - Minor
- **File & Location**: `frontend/src/utils/qrCode.ts`
- **Проблема**: При длине данных > 858 байт буфер молча обрезается до емкости версии 20-L, создавая поврежденный QR-код.
- **Решение**: Проверять длину перед кодированием и предупреждать о превышении лимита.

### [FE-11] Ложные срабатывания yamlValidator.ts на схемах tls://, quic:// и двоеточиях
- **Category**: Frontend Defect / Bug
- **Severity**: P2 - Medium
- **File & Location**: `frontend/src/utils/yamlValidator.ts`
- **Проблема**: Проверка первого двоеточия ложно падает на схемах `tls://`, `quic://` и IPv6-адресах без списка `-`.
- **Решение**: Пропускать проверку для стандартных схем протоколов и строк с кавычками.

### [FE-12] Отсутствие document.hidden в ConnectionsViewer.tsx и useCallback в App.tsx
- **Category**: Frontend Defect
- **Severity**: P3 - Minor
- **File & Location**: `frontend/src/components/ConnectionsViewer.tsx` и `frontend/src/App.tsx`
- **Проблема**: Поллинг сокетов каждые 2.5с нагружает процессор роутера в фоновой вкладке. В `App.tsx` слушатель `keydown` пересоздается на каждый рендер из-за отсутствия `useCallback` у `switchTab`.
- **Решение**: Добавить `if (document.hidden) return` и `useCallback(switchTab)`.
