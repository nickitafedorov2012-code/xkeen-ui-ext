# Handoff State — v1.16.15.0

## Цель
Гарантировать железный инвариант выбранного сервера: при выборе любого сервера пользователем всё, что идёт НЕ напрямую (`DIRECT`/`REJECT`), идёт через выбранный сервер (в т.ч. `🛡️ Whitelist` при активном БС), без гонок старта, потери синхронизации при `reload_config` и залипания сокетов.

## План шагов
- [DONE] Шаг 1: Фильтрация `disabled_providers` и порядок `PROXY -> Fastest -> Fallback` в `backend/src/routing/ipset.rs` (`apply_whitelist_rules`), очистка осиротевшего `whitelist.devices[].server` в `backend/src/api/routing/devices.rs` (`set_device_routing`) — `cargo check` passed (0 errors)
- [DONE] Шаг 2: Актуальная проверка `🛡️ Whitelist.now` и селективный сброс сокетов в `backend/src/failover/switcher.rs` (`sync_whitelist_on_switch`) + единое обновление `target_server` и `switch_server` в `backend/src/api/servers/mod.rs` + обновление `vkturn_manual_switch_keeps_existing_connections` в `backend/src/transaction.rs`
- [DONE] Шаг 3: Восстановление активных селекторов `PROXY` и `🛡️ Whitelist` при `reload_config` и переключении в `backend/src/mihomo.rs` (`desired_tunnel_server_for_cfg`, `restore_active_selectors_after_reload`, `switch_server_inner`) — `cargo check --tests` passed (0 errors)
- [DONE] Шаг 3.5: Хранение `LAST_SELECTED_TUNNEL_SERVER` и приоритет `desired_for_primary` для `🛡️ Whitelist` в `backend/src/mihomo.rs` + синхронизация `whitelist.target_server` в `backend/src/failover/probe.rs`
- [DONE] Шаг 4: Исправление порядка стартовой инициализации и 30-секундный сторож инварианта в `backend/src/main.rs` (`regression_bug_4` passed)
- [IN_PROGRESS] Шаг 5: Анти-регрессионные тесты, релиз `v1.16.15.0` и проверка на `192.168.2.1:1001`
