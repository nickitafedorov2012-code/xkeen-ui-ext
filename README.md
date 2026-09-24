<div align="center">

# 🛣️ XKeen Route

**Высокопроизводительная веб-панель раздельной маршрутизации и управления прокси для роутеров Keenetic / Netcraze с XKeen (Mihomo / Xray).**

[![Release](https://img.shields.io/github/v/release/nickitafedorov2012-code/xkeen-ui-ext?color=2B7FFF&label=Релиз&logo=github)](https://github.com/nickitafedorov2012-code/xkeen-ui-ext/releases)
[![License: AGPL-3.0](https://img.shields.io/badge/License-AGPL--3.0-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/Платформа-Entware%20(MIPS%20%7C%20ARM)-success.svg)](https://entware.net/)
[![Backend: Rust](https://img.shields.io/badge/Backend-Rust%20(Axum)-DEA584?logo=rust)](backend/)
[![Frontend: React](https://img.shields.io/badge/Frontend-React%20%2B%20TypeScript-61DAFB?logo=react)](frontend/)

[✨ Возможности](#-возможности) • [📸 Обзор интерфейса](#-обзор-интерфейса) • [⚡ Быстрая установка](#-быстрая-установка-entware) • [⚙️ Конфигурация](#️-конфигурация) • [🔧 Сборка](#-сборка-из-исходников)

</div>

---

## 📖 О проекте

**XKeen Route** — это современная панель управления для роутеров Keenetic и Netcraze с установленным пакетом [XKeen](https://github.com/jameszeroX/XKeen).

Она работает автономно прямо на роутере в среде **Entware** в виде единого бинарного файла, скомпилированного на Rust (минимальное потребление CPU и памяти, нулевое количество внешних зависимостей). Веб-интерфейс открывается в любом браузере в локальной сети на порту `1001`.

Формат хранения правил раздельной маршрутизации полностью совместим с десктопными и мобильными клиентами (*Keenetic Policy & XKeen Manager*), используя общие `AUTO-DEVICE` блоки в `/opt/etc/mihomo/config.yaml`.

---

## ✨ Возможности

- 🎨 **Быстрый и отзывчивый интерфейс**: глубокая тёмная тема, адаптивная верстка под десктоп и смартфоны, обновление данных по WebSocket и REST API.
- 📱 **Раздельная маршрутизация per-device**: назначение персонального сервера Mihomo или прямого выхода (`DIRECT`) индивидуально для каждого устройства в локальной сети.
- 📊 **Интерактивный дашборд**: живые графики трафика RX/TX, загрузка CPU/RAM роутера, статус ядра Mihomo и лента событий Failover в реальном времени.
- 🛰 **Серверы и провайдеры**: удобные карточки нод с протоколами (VLESS Reality, Shadowsocks, Hysteria2, Trojan и др.), замер задержки и выбор приоритетного узла.
- 🌐 **Монитор сетевых соединений**: отслеживание активных TCP/UDP сокетов в реальном времени с целевыми IP, хостами, скоростями и сработавшими правилами.
- 📋 **Инспектор правил и симулятор**: визуализация дерева маршрутизации Mihomo и моментальный тест маршрута для любого домена или IP.
- 🩺 **Диагностика системы**: автоматический мониторинг здоровья Entware, DNS over HTTPS (DoH), Keenetic RCI API и сетевых интерфейсов.
- 🤖 **Google AI & Antigravity Bypass**: выделенный режим раздельной маршрутизации и оптимизации задержки для сервисов Gemini, AI Studio и инструментов разработки.
- ⚡ **Автоматический Failover**: переключение на резервный сервер при превышении порога пинга и автоматический возврат на приоритетный узел при восстановлении.
- 💾 **Бэкапы и обновление в один клик**: управление конфигурационными файлами, резервное копирование и бесшовное обновление панели прямо из веб-интерфейса.

---

## 📸 Обзор интерфейса

### 📊 1. Дашборд — телеметрия и график сетевого трафика

Центральный пульт управления: мониторинг пропускной способности сети, графики входящей и исходящей скорости в реальном времени, загрузка ресурсов роутера Keenetic, статус ядра Mihomo и журнал событий автоматического переключения (Failover).

![Дашборд](docs/screenshots/dashboard.png)

---

### 📱 2. Устройства — раздельная маршрутизация per-device

Управление трафиком каждого клиента домашней сети из DHCP роутера. Назначение индивидуального прокси-сервера или прямого выхода, настройка лимитов скорости, отображение онлайн-статуса, фильтры и автоматическая отметка устройства, с которого открыта панель («ВЫ»).

![Устройства](docs/screenshots/devices.png)

---

### 🛰 3. Серверы и провайдеры — замер задержки и приоритеты

Полный каталог узлов из всех подключённых провайдеров (поддержка VLESS Reality, Shadowsocks, Hysteria2 и др.). Быстрый замер задержки (ping), переключение активного сервера, назначение приоритетного узла для Failover и управление черным списком.

![Серверы](docs/screenshots/servers.png)

---

### 🌐 4. Активные соединения — живой монитор сокетов

Интерактивный инспектор сетевых сессий в реальном времени: отображение хостов назначения, IP-адресов клиента и сервера, протокола, скорости передачи, общего объема данных и правила Mihomo, по которому направлен трафик.

![Активные соединения](docs/screenshots/connections.png)

---

### 📋 5. Инспектор правил и симулятор маршрутов

Наглядный просмотр всех действующих цепочек маршрутизации Mihomo и интерактивный симулятор: введите любой домен или IP-адрес, чтобы мгновенно узнать, через какой узел и на основании какого правила пойдет трафик.

![Инспектор правил и симулятор](docs/screenshots/rules.png)

---

### 🩺 6. Диагностика — комплексный аудит системы

Автоматическое тестирование всех ключевых компонентов: доступность Keenetic RCI API, статус ядра Mihomo, проверка резолва через DNS over HTTPS (DoH), мониторинг свободного места на накопителе Entware и состояние сетевых интерфейсов.

![Диагностика](docs/screenshots/diagnostics.png)

---

### 🤖 7. Google AI & Antigravity Bypass

Специализированный режим раздельной маршрутизации и оптимизации задержки для разработчиков и пользователей Google Gemini, Google AI Studio, Vertex AI и AI-ассистентов Antigravity.

![Google AI & Antigravity Bypass](docs/screenshots/google_ai.png)

---

### ⚙️ 8. Настройки — Failover, Keenetic RCI, AdBlock и резервные копии

Тонкая настройка системы: параметры автоматического перехода на резервный сервер при авариях, авторизация в Keenetic RCI (по токену `X-Ndma-Tkn` или паролю), интеграция с Mihomo, списки блокировки рекламы и создание/восстановление резервных копий конфигов в один клик.

![Настройки](docs/screenshots/settings.png)

---

## ⚡ Быстрая установка (Entware)

Установка выполняется одной командой по SSH на роутере Keenetic/Netcraze с установленным Entware:

### Стабильная версия (Latest):

```sh
curl -Ls https://raw.githubusercontent.com/nickitafedorov2012-code/xkeen-ui-ext/main/setup.sh | sh
```

### Бета-версия (Beta):

```sh
curl -Ls https://raw.githubusercontent.com/nickitafedorov2012-code/xkeen-ui-ext/main/setup.sh | sh -s -- beta
```

> [!TIP]
> **Если `raw.githubusercontent.com` недоступен или заблокирован провайдером**, используйте зеркало:
> ```sh
> curl -Ls https://ghproxy.net/https://raw.githubusercontent.com/nickitafedorov2012-code/xkeen-ui-ext/main/setup.sh | sh
> ```

При запуске через терминал SSH скрипт откроет интерактивное меню (установка, обновление, удаление).

---

### 🕹 Управление сервисом

После установки панель доступна в браузере по адресу: **`http://<IP-роутера>:1001`** (например, `http://192.168.1.1:1001`).

Управление через системный init-скрипт Entware:
```sh
sh /opt/etc/init.d/S99xkeen-route start    # Запустить панель
sh /opt/etc/init.d/S99xkeen-route stop     # Остановить панель
sh /opt/etc/init.d/S99xkeen-route restart  # Перезапустить панель
sh /opt/etc/init.d/S99xkeen-route status   # Проверить статус процесса
```

---

### 🗑 Удаление

- **Удаление бинарника и службы (конфигурация сохраняется):**
  ```sh
  curl -Ls https://raw.githubusercontent.com/nickitafedorov2012-code/xkeen-ui-ext/main/setup.sh | sh -s -- uninstall
  ```
- **Полное удаление (вместе со всеми файлами конфигураций):**
  ```sh
  curl -Ls https://raw.githubusercontent.com/nickitafedorov2012-code/xkeen-ui-ext/main/setup.sh | sh -s -- uninstall purge
  ```

---

## ⚙️ Конфигурация

Файл конфигурации расположен по пути `/opt/etc/xkeen-route/config.json`. Поддерживается deep-merge (неуказанные параметры автоматически берутся из дефолтных значений):

```json
{
  "rci": {
    "host": "127.0.0.1",
    "port": 79,
    "login": "admin",
    "password": "",
    "token": ""
  },
  "mihomo": {
    "host": "127.0.0.1",
    "port": 9090,
    "secret": ""
  },
  "failover": {
    "enabled": false,
    "ping_threshold_ms": 300,
    "priority_server": "",
    "auto_restore_priority": true,
    "interval_secs": 60
  },
  "refresh_interval_sec": 10
}
```

> [!NOTE]
> **Авторизация в RCI**: панель автоматически подхватывает токен авторизации `X-Ndma-Tkn` из `/opt/etc/xkeen/xkeen.json`. Если токен не задан, используется challenge-auth по связке login/password.

---

## 🔧 Сборка из исходников

### 1. Сборка фронтенда (Node.js 18+):
```sh
cd frontend
npm install
npm run build
```

### 2. Сборка бэкенда (Rust 1.75+):
```sh
cd backend
cargo build --release
```

### 3. Локальная разработка (с Hot-Reload):
```sh
# Терминал 1: Запуск бэкенда на порту 1001
cd backend && cargo run

# Терминал 2: Запуск дев-сервера Vite с проксированием /api -> :1001
cd frontend && npm run dev
```

### 4. Кросс-компиляция под роутеры (MIPS / MIPSle / ARM64):
Кросс-компиляция выполняется через инструмент [`cross`](https://github.com/cross-rs/cross) или автоматически в GitHub Actions (workflow [`.github/workflows/build.yml`](.github/workflows/build.yml)):
```sh
cross build --target mipsel-unknown-linux-musl --release
cross build --target mips-unknown-linux-musl --release
cross build --target aarch64-unknown-linux-musl --release
```

---

## ⚠️ Безопасность

Панель **не содержит встроенной аутентификации** (осознанное решение для минимизации накладных расходов в локальной сети) и предназначена **исключительно для доверенной локальной сети**.

> [!CAUTION]
> Не пробрасывайте порт `1001` в публичный интернет напрямую. Для удалённого доступа используйте безопасное VPN-подключение к роутеру (WireGuard / OpenVPN / IPsec) либо KeenDNS с настроенной защитой паролем.

---

## 🤝 Совместимость и благодарности

Проект вдохновлен и совместим с экосистемой XKeen:
- [zxc-rv/XKeen-UI](https://github.com/zxc-rv/XKeen-UI) — исходные архитектурные идеи
- [Skrill0/XKeen](https://github.com/Skrill0/XKeen) и [jameszeroX/XKeen](https://github.com/jameszeroX/XKeen) — пакет XKeen для Keenetic
- [Anonym-tsk/nfqws-keenetic](https://github.com/Anonym-tsk/nfqws-keenetic) — утилиты обхода и оптимизации

---

## 📖 Документация для разработчиков

Внутренняя документация, грабли Mihomo/YAML/кодировок, полный ченжлог и заметки об окружении вынесены в [DEVELOPMENT.md](DEVELOPMENT.md).

---

## 📄 Лицензия

Проект распространяется под свободной лицензией **GNU Affero General Public License v3.0 (AGPL-3.0)**. См. файл [LICENSE](LICENSE) для подробностей.

Copyright (C) 2026 nickitafedorov2012-code

