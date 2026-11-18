# Handoff State — Каталог РФ (удаление 153 неработающих в DIRECT доменов)

## Цель
Удалить 153 не открывающихся напрямую (`DIRECT`) домена из встроенного каталога РФ (`DOMESTIC_WHITELIST_DOMAINS` / `DOMESTIC_CATALOG`, 402 -> 249), исключать `proxy_catalog_domains` из `apply_whitelist_dns`, отдавать `proxy_catalog_domains` в `GET /api/whitelist`, обновить тесты и применить список на живом роутере `192.168.2.1:1001`.

## План шагов
- [DONE] Шаг 1: Очистка встроенного каталога в `backend/src/whitelist_catalog.rs` (`DOMESTIC_WHITELIST_DOMAINS`, `DOMESTIC_CATALOG`, тест `test_mobile_infrastructure_and_cdns_present`: 402 -> 249 доменов; тесты `whitelist_catalog::tests` passed)
- [DONE] Шаг 2: Исключение `proxy_catalog_domains` в `backend/src/routing/ipset.rs` (`apply_whitelist_dns`) и возврат `proxy_catalog_domains` в `backend/src/api/routing/whitelist.rs` (`WhitelistStatusResponse`, `get_whitelist`) — `cargo check --tests` passed
- [DONE] Шаг 3: Обновление тестов и добавление анти-регрессионного теста `regression_pruned_153_non_connecting_direct_domains_and_dns_policy` в `backend/src/routing/mod.rs` (`routing::tests`: 116 passed, 0 failed)
- [DONE] Шаг 4: Бэкап на `192.168.2.1:1001` (`xr-20261114-234503-757`), применение `proxy_catalog_domains` (153 адреса переведены из `DIRECT` в `🛡️ Whitelist`, `PROXY` и `🛡️ Whitelist` сохранены на `📞 VK-туннель [БС]`), `cargo test` (738 passed, 0 failed) и `npm --prefix frontend run build` (`tsc && vite build`, built in 2.78s)
- [DONE] Шаг 5.1: Bump версии до `1.16.16` / `v1.16.16.0` (`backend/Cargo.toml`, `backend/Cargo.lock`, `frontend/package.json`), ченжлог в `DEVELOPMENT.md`, `cargo check` и `npm --prefix frontend run build` (`built in 2.15s`)
- [DONE] Шаг 5.2: Коммит `v1.16.16.0`, тег `v1.16.16.0`, push в `origin main`, ожидание GitHub Release CI
- [DONE] Шаг 5.3: Установка `v1.16.16.0` на `192.168.2.1:1001` (`POST /api/backups` `xr-20261114-235851-090`, `POST /api/update/install`), очистка временного `proxy_catalog_domains` (`[]`), проверка каталога (249), Mihomo правил (0 из 153 в `DIRECT`) и активного туннеля (`📞 VK-туннель [БС]`)

## Исправление косяков (`КОСЯКИ.md`)
- [DONE] Косяк 1: Узлы подписок пингуются через `/providers/proxies/{provider}/{node}/healthcheck`, убран безусловный `/group/PROXY/delay` (`backend/src/mihomo.rs`, `mihomo::tests` 45/45 passed)
- [DONE] Косяк 2: `sync_disabled_providers_in_yaml` отключает `include-all: true`/`include-all-providers: true` при наличии `disabled_providers` и восстанавливает при включении (`backend/src/routing/validator.rs`, `routing::tests` 117/117 passed)
- [DONE] Косяк 3: `POST /api/servers/ping` и `ping_all_url` пропускают игнор-лист и служебные группы, пулы берут задержку от активного листа без сетевого шторма, UI передаёт `include_pools: showPools` (`backend/src/api/servers/mod.rs`, `backend/src/mihomo.rs`, `frontend/src/components/Servers.tsx`, `api::servers::tests` 6/6 passed)
- [DONE] Косяк 4: Добавлена singleflight-блокировка `PROVIDER_PROXIES_FETCH_LOCK` для `get_provider_proxies`, 12 параллельных вызовов схлопываются в 1 запрос (`backend/src/mihomo.rs`, `mihomo::tests` 46/46 passed)
- [DONE] Косяк 5: Добавлена координация `begin_manual_ping` / `run_auto_ping_tick` (20s cooldown) между ручным пингом и фоновым `PING-AUTO` (`backend/src/mihomo.rs`, `backend/src/api/servers/mod.rs`, `backend/src/main.rs`, `mihomo::tests` 47/47 passed)
- [DONE] Косяк 6: `store_ping_cache` удаляет устаревший кэш при `-1`, `Servers.tsx` синхронизирует `fresh.ping_ms` без залипания (`backend/src/mihomo.rs`, `frontend/src/components/Servers.tsx`, `frontend/src/components/Servers.test.tsx`, `mihomo::tests` 48/48 + `Servers.test.tsx` 2/2 passed)
- [DONE] Косяк 7: Жёсткая изоляция маршрутизации в режиме БС (`backend/src/routing/ipset.rs`, `backend/src/routing/iptables.rs`, `backend/src/routing/mod.rs`, `routing::tests` 118/118 passed)
- [DONE] Косяк 8: Отклонение IP/CIDR в `is_valid_rule_domain` и генерация `IP-CIDR,<cidr>,<target>,no-resolve` (`backend/src/routing/validator.rs`, `backend/src/routing/policy_routing.rs`, `backend/src/routing/ipset.rs`, `routing::tests` 119/119 passed)
- [DONE] Косяк 9: Исправление слипания IP разных MAC в `whitelist.devices` и `gaming.devices` (`backend/src/watchdog.rs`, `watchdog::` 42/42 passed)

## Исправление маршрутизации прямых доменов в режиме Белых списков (БС)
- [DONE] Шаг 1: Восстановить объединение `DOMESTIC_WHITELIST_DOMAINS` (249 доменов) + `whitelist.custom_domains` + `cfg.direct_domains` (за вычетом `proxy_catalog_domains`) в `explicit_whitelist_direct_domains`, `apply_whitelist_rules` (включая `IP-CIDR` из обоих списков) и `apply_whitelist_dns` в `backend/src/routing/ipset.rs`.
- [DONE] Шаг 2: Вычислять локальный DNS-эндпоинт с учётом AdGuard Home в `backend/src/routing/iptables.rs` (`apply_routing_with_filters`).
- [DONE] Шаг 3: Обновить `regression_global_whitelist_isolates_rules_and_is_reversible` и добавить `regression_whitelist_routes_catalog_and_user_domains_direct_above_catchall` в `backend/src/routing/mod.rs`.
- [DONE] Шаг 4: Запустить `cargo test` в `backend`, проверить 100% прохождение тестов и запросить разрешение на коммит и деплой (`748 passed, 0 failed`).


## Реализация «Подключить бесплатную мульти-подписку в 1 клик» (VPN / Белые списки)
- [DONE] Шаг 1: Зафиксирован архитектурный план.
- [DONE] Шаг 2: Расширение `backend/src/subscription_parser.rs` (`is_forbidden_ip`, `validate_proxy_host_port`, `sanitize_proxy_value`, `node_dedup_key`, `parse_subscription_nodes_bounded`, `proxies_to_mihomo_yaml`) и исключение служебного провайдера `__free_sub_probe` в `backend/src/routing/validator.rs`, `ipset.rs`, `iptables.rs`, `api/routing/devices.rs`, `backend/src/mihomo.rs` (+ `probe_service_provider_node_l7` и `parse_l7_healthcheck_response`).
- [DONE] Шаг 3: Реализация `backend/src/free_sub/mod.rs` (компактные источники + RU-зеркала `gitverse.ru` + туннель + локальный seed-кэш на диске, жёсткие лимиты, DNS-резолв с отсевом запрещённых IP, мягкая квота `/24`, конвейер Fast-path -> TCP 16 сокетов/rate-limit/700ms+1800ms -> L7 через `__free_sub_probe`, атомарная публикация в `free_vpn.yaml`/`free_wl.yaml` с сохранением активной сессии) + эндпоинты `POST /api/providers/free-sub/connect` и `GET /api/providers/free-sub/status` в `backend/src/api/servers/subscriptions.rs` и `router.rs`.
- [DONE] Шаг 4: Реализация UI-компонента `frontend/src/components/servers/FreeSubCard.tsx`, интеграция в `frontend/src/components/Servers.tsx`, типы в `frontend/src/types.ts`, тесты `FreeSubCard.test.tsx`.
- [DONE] Шаг 5: Полный прогон тестов (`cargo test` — 759 passed, 0 failed; `FreeSubCard.test.tsx` + `Servers*.test.tsx` — 8 passed; `npm run build` — ок) и визуальная проверка скриншота `free_sub_servers.png` через `read_file`.
- [DONE] Шаг 6: Релиз `v1.17.0-beta.1` (коммит `1975b8b` в `dev`, тег `v1.17.0-beta.1`, CI `43724748335` success), бэкап `xr-20261115-143129-299` и установка на `192.168.2.1:1001` (`version=v1.17.0-beta.1`, `running=True`, `free_sub_status=True`).

## Баг-ревью (`docs/REWORK_FIRST.md`)
- [DONE] Шаг 0–1: Перепроверены по рабочему коду и тестам пункты `1, 13, 36, 37, 70, 91, 114` и предупреждения компилятора (0 warnings, `cargo test` 759/759 passed). Зафиксированы 2 предсуществующих падения фронтенд-тестов от `v1.16.21` (`versionUpdate.test.ts:20`, `Header.test.tsx:248`).
- [DONE] Шаг 2: Перепроверены пункты `4–10, 12, 14, 17, 24–30, 32, 33, 39, 41, 51, 56–59, 62, 64, 67, 76, 77, 80–86, 94–96, 98, 108–111, 116, 118–120, 122, 130, 131, 133–136`. Дозакрыт остаток по Багу 6 в `frontend/src/components/Gaming.tsx` (`handleSave` больше не шлёт устаревший `enabled` при сохранении доменов/платформ, тест в `Gaming.test.tsx` усилен, `Gaming.test.tsx` + `ConflictPairs.test.tsx` 16/16 passed).
- [DONE] Шаг 3: Неначатые пункты и остатки проверены; исправлены [free_sub/mod.rs](backend/src/free_sub/mod.rs) (`sync_whitelist_mode_tunnel` + `test_6`) и 2 предсуществующих падения фронтенд-тестов (`versionUpdate.test.ts:20`, `Header.test.tsx:209`). Итоговый прогон: `cargo test` 760/760 passed, `vitest run` 44/44 файлов (381/381 тестов) passed, `vite build` ok. Выпуск `v1.17.0-beta.2`.

## Переработка механики и UI «Бесплатных мульти-подписок» (v1.17.0-beta.6)
- [DONE] Шаг 1: `backend/src/routing/validator.rs` — добавлена `add_provider_to_proxy_groups` (гарантированное добавление `free_vpn`/`free_wl` в `use:` группы `PROXY`) + тест `regression_add_provider_to_proxy_groups_adds_to_proxy`.
- [DONE] Шаг 2: `backend/src/mihomo.rs` — Stability Gate (`probe_service_provider_node_l7_stable`: `generate_204` + `cdn-cgi/trace`), `get_connections_count` и виртуальные карточки `free_vpn` / `free_wl` в `get_servers`.
- [DONE] Шаг 3: `backend/src/free_sub/mod.rs` — `BACKGROUND_BACKUP_TARGET = 20`, разнообразие по IP:port, `auto_connect: bool`, 24ч blacklist TTL, per-node история сессий и трафика, Real-Traffic Fast-Path, Smart Watchdog, `enable_both_free_subs`, `disconnect_free_sub`, `remove_free_sub`, `switch_pool_node`, `blacklist_and_next`, `start_background_refill`, `get_pool_and_history`.
- [DONE] Шаг 4: `backend/src/api/servers/subscriptions.rs`, `backend/src/api/servers/mod.rs`, `backend/src/api/router.rs`, `backend/src/main.rs` — эндпоинты и 5-секундный цикл Smart Watchdog.
- [DONE] Шаг 5: `frontend/src/` — кнопка `🆓 Добавить бесплатные подписки` в шапке `Servers.tsx`, единые карточки в `ServerList.tsx` с «Отключить» / «Удалить» / «Узлы и история ▾», компонент `FreeSubPoolSubmenu.tsx`, удаление верхнего `FreeSubCard`.
- [DONE] Шаг 6: Автотесты бэкенда и фронтенда (`cargo test`, `npm test`, `npm run build`).

## Исправление багов из ревью (`review-2026-11-16/REVIEW.md` + живая проверка `192.168.1.1`)
- [DONE] Батч 1 (`free_sub` + `mihomo` + `ServerList` UI): R11–R21, R26–R33 + сброс `PROXIES_CACHE` в `publish_and_connect` + live-прогресс в UI. Файлы: `backend/src/free_sub/mod.rs`, `backend/src/api/servers/mod.rs`, `backend/src/api/servers/subscriptions.rs`, `backend/src/mihomo.rs`, `frontend/src/components/servers/FreeSubPoolSubmenu.tsx`, `frontend/src/components/servers/ServerList.tsx`.
- [DONE] Батч 2 (`backups.rs` + `system_ssh.rs` + `gaming.rs` + `override_sync.rs`): R01–R10, R53, R54 + регрессионные тесты.
- [DONE] Батч 3 (`subscription_parser.rs` + `transaction.rs`): R22–R25, R48 + регрессионные тесты.
- [DONE] Батч 4 (`watchdog.rs` + `devices.rs` + `traffic.rs` + `auth.rs` + `antigravity.rs` + `updater.rs` + `MihomoCoreModal.tsx`): R34–R47, R49–R52 + регрессионные тесты.



## Улучшение детектора белых списков для мобильной сети (Mobile Whitelist Detector v2)
- [DONE] Шаг 1: Реализованы улучшения в `backend/src/config.rs`, `backend/src/failover/probe.rs`, `backend/src/failover/mod.rs`, `backend/src/api/servers/mod.rs` (двухконтурный маяк DIRECT РФ vs зарубеж, независимый от `failover.enabled` запуск, гистерезис 2+ подтверждений + 15-сек ускорение дожима, LTE-таймаут до 3000–3500 мс, `manual_cooldown_mins`, fallback на `bs_candidate_ids`, анти-регрессионные тесты: `failover` 131/131 passed).
- [DONE] Шаг 2: Обновлён UI в `frontend/src/types.ts` и `frontend/src/components/settings/WhitelistDetectorSection.tsx` (`Settings.test.tsx` 16/16 passed, `vite build` ok).
- [DONE] Шаг 3: Бэкап на `192.168.1.1:1001` (`xr-20261118-163330-221`), сборка `aarch64-unknown-linux-musl` и проверка на тестовом роутере `192.168.1.1` (двухконтурный маяк `domestic_ok=true`, гистерезис `1/2 -> 2/2`, выбор `free_wl`). Настройки и `detector_state.json` откачены в исходное состояние.
