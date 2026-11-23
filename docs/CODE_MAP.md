# XKeen Route — Карта кода и канонические точки входа

## 1. Канонические точки входа

- **Чтение `AppConfig`**: `state.cfg_snapshot().await` (`AppState::cfg_snapshot` в `backend/src/main.rs`) — мгновенный `Arc<AppConfig>` без удержания `RwLockReadGuard` через `.await`.
- **Запись только `config.json`**: `state.update_config(|cfg| ...).await`, `state.try_update_config(...)` или `state.try_update_config_nonblocking(...)` (`backend/src/main.rs`).
- **Транзакция `config.yaml` (+ `config.json`)**: `ConfigTx::begin(&state).await` (`backend/src/transaction.rs`) или `api::commit_config_with_routing(&state, |cfg| ...)` (`backend/src/api/mod.rs`).
- **Переключение сервера**: `api::servers::execute_server_switch_workflow(&state, server_id)` (`api/servers/mod.rs`) -> `mihomo::switch_server` -> `failover::handle_whitelist_transition` -> `api::sync_whitelist_mode_tunnel`.
- **Виртуальные пулы `free_vpn` / `free_wl`**: `free_sub::{enable_both_free_subs, spawn_one_click_connect, spawn_background_refill, switch_pool_node, blacklist_and_next, disconnect_free_sub, remove_free_sub, get_status_for, get_all_statuses, reset_free_sub_runtime_and_cache}` (`backend/src/free_sub/mod.rs`).
- **Режим БС (`XKEEN_BS`)**: `api::routing::whitelist::{save_whitelist, set_whitelist_strategy, toggle_whitelist_device}` + `routing::ipset::{apply_whitelist_rules, apply_whitelist_dns}` + `api::sync_full_tunnel_modes`.
- **Игровой режим (`XKEEN_GAMING`)**: `api::routing::gaming::{save_gaming, toggle_gaming, sync_full_tunnel_modes}` + `watchdog::apply_smart_gaming_toggle`.
- **AdGuard Home (`53 -> 5353`)**: `api::dns_adguard::adblock_toggle_core` + `adguard::{iptables, service, yaml}` + `api::resolve_active_dns_port`.
- **Zapret (`S51zapret`)**: `api::zapret::presets::{zapret_set_enabled_core, sync_zapret_files}` + `api::zapret::diagnostics::{zapret_action, toggle_device_zapret}` + `zapret::ZapretLockGuard`.
- **VK-туннель (`vkturn` / `csq0`)**: `api::vkturn::*` -> `vkturn::VkTurnManager` (`vkturn/{mod,auth,tunnel,health}.rs`).
- **`geo_override` и DNS-порт**: `override_sync::sync_from_config(&cfg, extra_cdns)` (единственный источник правды; `sync_geo_override` напрямую не вызывать); `api::resolve_active_dns_port(&cfg)`.
- **Сквозная цепочка I4**: `ConfigTx` -> `mihomo::reload_config` -> `mihomo::restore_active_selectors_after_reload` -> `api::resolve_active_dns_port` -> `api::sync_full_tunnel_modes` -> `mihomo::invalidate_proxies_cache` + `override_sync::sync_from_config`.
- **Порядок блокировок I3**: `config_lock` -> `routing_lock` -> `zapret_lock` -> `speedtest_lock` (не удерживать во время долгих HTTP/TCP/L7/CDN операций).

## 2. Карта модулей `backend/src`

- `main.rs` — точка входа, CLI, `AppState` (`cfg_snapshot`, `update_config*`), фоновые задачи, роутер.
- `config.rs` — модели `AppConfig`, загрузка/сохранение `config.json`, валидация IP/MAC/доменов.
- `transaction.rs` — `ConfigTx` (атомарный commit/rollback `config.json` + `config.yaml`, `PendingCommit`).
- `mihomo.rs` — REST-клиент Clash/Mihomo, переключение, пинги (`PROXIES_CACHE`, `PROVIDER_PROXIES_CACHE`, `PING_CACHE`, `PROVIDER_PROXIES_FETCH_LOCK`, `MANUAL_PING_IN_FLIGHT`).
- `routing/{mod,ipset,iptables,dns_redirect,policy_routing,policy_isolation,validator}.rs` — генерация `config.yaml`, `ipset`/`iptables`, изоляция политик (`REDIR_PORT_FALLBACK_WARNED`, `DEFAULT_POLICY_IPS`).
- `failover/{mod,probe,quarantine,scoring,switcher,history,types}.rs` — автопереключение, детектор БС, карантин (`CHECK_IN_PROGRESS`, `LAST_SWITCH`, `SERVER_QUARANTINE`, `DETECTOR_STATE`, `CANARY_STAGE`, `WHITELIST_EPISODE_BYTES`).
- `free_sub/mod.rs` — бесплатные пулы `free_vpn`/`free_wl` (`FREE_VPN_STATUS`, `FREE_WL_STATUS`, `FREE_VPN_RUN_LOCK`, `FREE_WL_RUN_LOCK`, `FREE_SUB_PROBE_LOCK`, `FREE_SUB_CACHE_LOCK`, `FREE_VPN_GEN`, `FREE_WL_GEN`).
- `subscription_parser.rs` — парсер VLESS/VMess/SS/Trojan, валидация хостов и запрещённых IP.
- `override_sync.rs` — DNS-резолв `geo_override` и авто-CDN кэш (`OVERRIDE_CACHE`, `CACHED_AUTO_CDNS`, `IPSET_SYNC_LOCK`).
- `cdn_discovery.rs` — ручной/плановый поиск CDN (`PROXIED_CLIENT`).
- `adguard/{mod,yaml,iptables,service}.rs` — управление AdGuard Home и редиректом `53 -> 5353`.
- `zapret.rs` — межпроцессный lock/состояние Zapret (`CURRENT_OPERATION_STATE`).
- `vkturn/{mod,auth,tunnel,health}.rs` — супервизор `vk-turn`/`csq0`, авторизация, MTU/TCPMSS, healthcheck.
- `watchdog.rs` — сторож Mihomo/Zapret/Smart Gaming/DHCP (`LAST_OWN_YAML_HASH`, `RECENT_GAMING_CONNS`).
- `traffic.rs` / `direct_traffic.rs` — живые счётчики трафика (`PROXY_DOWN`/`DIRECT_DOWN`/`WHITELIST_DOWN`, `DIRECT_IPTABLES_ACTIVE`).
- `auth.rs` — сессии, PBKDF2, CSRF (`FAILED_LOGIN_ATTEMPTS`, `REVOKED_TOKENS`).
- `updater.rs` — самообновление, каналы `stable`/`beta`, маркер отката (`UPDATE_LOCK`).
- `rci.rs` — клиент KeeneticOS RCI (`AUTHED`, `TOKEN_OK`, `AUTH_LOCK`, `ROUTER_IP_CACHE`).
- `bot.rs` / `notifications.rs` — Telegram-бот и уведомления.
- `antigravity.rs` / `speedtest.rs` / `system.rs` / `logger.rs` / `frontend.rs` / `shutdown.rs` / `error.rs` — обход AGY, замер скорости, метрики CPU, кольцевой лог (`LOG_TX`, `RING_BUFFER`), встроенный `Assets`, graceful shutdown (`GLOBAL_SHUTDOWN`).
- `whitelist_catalog.rs` / `whitelist_tags.rs` — встроенный каталог доменов РФ (249) и теги БС.
- `api/{mod,router,status,backups,clash_proxy,dns_adguard,domains,system_ssh,vkturn}.rs` — HTTP API слой.
- `api/routing/{mod,devices,gaming,whitelist}.rs` — API маршрутизации устройств, игрового режима и БС.
- `api/servers/{mod,subscriptions}.rs` — API серверов, Google Flow, подписок и `free_sub`.
- `api/zapret/{mod,firewall,presets,diagnostics}.rs` — API Zapret и DPI-аналитика (`ZAPRET_LAST_IPT_BYTES`, `ZAPRET_PERIOD_SAVED_BYTES`, `ZAPRET_LAST_FLUSHED_BYTES`).

## 3. Карта модулей `frontend/src`

- `App.tsx` / `api.ts` / `types.ts` — корневой шелл, вкладки, `apiGet`/`apiPost`/`apiPut`/`apiDelete`, общие TS-типы.
- `components/{Header,Dashboard,Servers,Whitelist,Devices,Gaming,Zapret,AdGuard,VkTurn,Antigravity,Diagnostics,Settings,Help}.tsx` — экраны вкладок и шапка.
- `components/servers/{ServerList,ActiveServerBanner,FreeSubCard,FreeSubPoolSubmenu,SubscriptionsModal,IgnoreListModal,serversHelpers}.tsx` — карточки серверов, пулы `free_sub`, подписки.
- `components/{TrafficGraph,ConnectionsViewer,RulesViewer,PoliciesMap,LogsViewer,TaskManager,SelfDiagnosticsCard,XboxDns}.tsx` — графики, соединения, правила, логи и диагностика.
- `components/{ConfigEditor,DeviceRoutingModal,DeviceScheduleModal,FlowRepairModal,MihomoCoreModal,ZapretCoreModal,OutboundGeneratorModal,PresetCatalogModal,ShareNodeModal,UpdateModal,LoginModal}.tsx` — модальные окна.
- `components/settings/*`, `components/gaming/*`, `components/zapret/*`, `components/vkturn/*` — секции настроек, игрового режима, Zapret и VK-туннеля.
- `utils/*`, `hooks/useUpdateCheck.ts`, `constants/presets.ts`, `styles/*` — утилиты, хук обновлений, пресеты и CSS-темы.

## 4. Запрещённые анти-паттерны

- Держать `state.config.read().await` через `.await` — только `state.cfg_snapshot().await`.
- Вызывать `*state.config.write().await` или `config.write().await` напрямую вне `transaction.rs` и `AppState::update_config*`.
- Записывать `config.json` из GET-хэндлеров без `config_lock` / атомиков (пример: счётчик Zapret).
- Менять `config.yaml` в обход `ConfigTx` / `commit_config_with_routing`.
- Вызывать `override_sync::sync_geo_override` напрямую вместо `override_sync::sync_from_config`.
- Запускать сетевой поиск CDN синхронно в `POST /api/domains` (Instant Domain Save).
- Нарушать порядок блокировок `config_lock -> routing_lock -> zapret_lock -> speedtest_lock`.
- Вставлять блоки `#[cfg(test)]` посреди рабочего кода (только в конце файла или отдельном `tests.rs`).
