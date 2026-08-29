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

```sh
curl -Lsfo /tmp/xr-setup.sh <URL>/setup.sh && sh /tmp/xr-setup.sh
```

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
