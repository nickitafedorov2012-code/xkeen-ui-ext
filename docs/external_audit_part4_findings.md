# Результаты аудита Part 4 (Кодовая база XKeen Route)
> Зафиксировано из внешнего анализа нейросети для компонентов фронтенда (React 18 / TypeScript).

## ⚡ Раздел 1: Рассинхронизация контрактов API и потеря данных (Frontend-Backend Desync)

### [FE-01] Использование s.name вместо s.id в DeviceScheduleModal
- **Category**: Frontend API Desync / Bug
- **Severity**: P0 - Critical
- **File & Location**: `frontend/src/components/Devices.tsx` (строка 653) и `frontend/src/components/DeviceScheduleModal.tsx` (строки 24, 248–252)
- **Проблема**: В `DeviceScheduleModal` передаются очищенные отображаемые имена серверов (`s.name`) без флагов/эмодзи вместо оригинальных идентификаторов `s.id`. Mihomo требует точное имя ноды, поэтому при срабатывании расписания переключение падает с ошибкой `proxy not found`.
- **Решение**: Передавать массив `{ id, name }`, сохранять `s.id` в `target_server`.

### [FE-02] Потеря expected_revision и затирание настроек в Settings.tsx
- **Category**: State Desync / Bug
- **Severity**: P1 - High
- **File & Location**: `frontend/src/components/Settings.tsx` (`save`, `saveDomains`, `handleApplyPreset`)
- **Проблема**: Поле `revision` игнорируется при загрузке доменов и не передается в `POST /api/domains` (`expected_revision`), отключая защиту от конфликтов правок. В `save()` поля Telegram/Webhook не берутся из локального стейта, перезаписывая настройки старыми значениями.
- **Решение**: Передавать `expected_revision` и синхронизировать стейт оповещений в объекте `settings`.

### [FE-03] Несовпадение ответа POST /api/notifications/test
- **Category**: Frontend API Desync
- **Severity**: P2 - Medium
- **File & Location**: `frontend/src/components/Settings.tsx` (`handleTestNotification`) vs `backend/src/api.rs` (`test_notification`)
- **Проблема**: Фронтенд ждет `res.message`, а бэкенд возвращает `res.results` (массив со статусами). В итоге при ошибке Telegram в UI всегда отображается зеленое сообщение об успехе.
- **Решение**: Парсить `res.results`, проверять наличие символа `❌` в результатах.

### [FE-04] Игнорирование SIGKILL (-9) и устаревших чекбоксов Gaming
- **Category**: Frontend API Desync
- **Severity**: P2 - Medium
- **File & Location**: `frontend/src/components/TaskManager.tsx` и `frontend/src/components/Gaming.tsx`
- **Проблема**: В `TaskManager.tsx` кнопка `SIGKILL (-9)` шлет сигнал `KILL`, но бэкенд жестко использует `-15` (SIGTERM). В `Gaming.tsx` чекбоксы `smart_split` и `fix_nat_fake_ip` помечены как устаревшие и не влияют на маршрутизацию.
- **Решение**: Убрать или синхронизировать кнопку SIGKILL, скрыть устаревшие флаги в Gaming.

### [FE-05] Битая ссылка скачивания расширения в FlowRepairModal
- **Category**: Frontend API Desync / Bug
- **Severity**: P2 - Medium
- **File & Location**: `frontend/src/components/FlowRepairModal.tsx` (строка 25) vs `backend/src/main.rs`
- **Проблема**: Зашит путь `/flow-unlock-extension.zip` вместо `/api/flow/extension.zip`.
- **Решение**: Использовать путь `/api/flow/extension.zip`.

---

## 🐛 Раздел 2: Баги React-хуков, Stale Closures и утечки ресурсов

### [FE-06] Неработающий поиск (Ctrl+F) в ConfigEditor.tsx
- **Category**: Frontend Defect / Bug
- **Severity**: P2 - Medium
- **File & Location**: `frontend/src/components/ConfigEditor.tsx` (строки 52, 240–252)
- **Проблема**: Переменная `searchTerm` нигде не используется после ввода, переход к совпадению и подсветка отсутствуют.
- **Решение**: Реализовать функцию `findNext` с вызовом `textarea.setSelectionRange()`.

### [FE-07] Фоновые запросы к GitHub при закрытых модальных окнах
- **Category**: Frontend Defect / Performance
- **Severity**: P2 - Medium
- **File & Location**: `frontend/src/components/MihomoCoreModal.tsx` и `frontend/src/components/UpdateModal.tsx`
- **Проблема**: В `useEffect` нет проверки `if (!isOpen) return`, поэтому при загрузке страницы панели сразу выполняются тяжелые запросы к GitHub API, даже если модалки не открыты.
- **Решение**: Обернуть вызовы в `if (isOpen)` и добавить `isOpen` в зависимости.

### [FE-08] Stale Closure в Devices.tsx и утечка таймера в UpdateModal.tsx
- **Category**: Frontend Defect
- **Severity**: P3 - Minor
- **File & Location**: `frontend/src/components/Devices.tsx` (`sortDevices`) и `frontend/src/components/UpdateModal.tsx`
- **Проблема**: В `sortDevices` пропущен `trafficMap` в зависимостях `useCallback`, ломая сортировку при динамическом обновлении трафика. В `UpdateModal.tsx` интервал не очищается при размонтировании.
- **Решение**: Добавить `trafficMap` в зависимости хука, сохранять таймер в ref.

---

## 🔍 Раздел 3: Хардкод во фронтенде (Frontend Hardcode Detection)

### [HC-09] Захардкоженные порты, IP и устаревшие описания
- В `Dashboard.tsx`: текст `Mixed Port: 7890`, фоллбэк `127.0.0.1:79`.
- В `LogsViewer.tsx`: жестко зашиты `порт 1001` и `порт 9090`.
- В `Antigravity.tsx`: fallback IP `192.168.2.1` и порт `1001`.
- В `Settings.tsx`: текст о сессии `30 дней` (в коде сессия 24 часа).
- В `TrafficGraph.tsx`: отсутствие проверки `document.hidden` в секундном таймере.
