# Handoff State — Каталог РФ (удаление 153 неработающих в DIRECT доменов)

## Цель
Удалить 153 не открывающихся напрямую (`DIRECT`) домена из встроенного каталога РФ (`DOMESTIC_WHITELIST_DOMAINS` / `DOMESTIC_CATALOG`, 402 -> 249), исключать `proxy_catalog_domains` из `apply_whitelist_dns`, отдавать `proxy_catalog_domains` в `GET /api/whitelist`, обновить тесты и применить список на живом роутере `192.168.2.1:1001`.

## План шагов
- [DONE] Шаг 1: Очистка встроенного каталога в `backend/src/whitelist_catalog.rs` (`DOMESTIC_WHITELIST_DOMAINS`, `DOMESTIC_CATALOG`, тест `test_mobile_infrastructure_and_cdns_present`: 402 -> 249 доменов; тесты `whitelist_catalog::tests` passed)
- [DONE] Шаг 2: Исключение `proxy_catalog_domains` в `backend/src/routing/ipset.rs` (`apply_whitelist_dns`) и возврат `proxy_catalog_domains` в `backend/src/api/routing/whitelist.rs` (`WhitelistStatusResponse`, `get_whitelist`) — `cargo check --tests` passed
- [DONE] Шаг 3: Обновление тестов и добавление анти-регрессионного теста `regression_pruned_153_non_connecting_direct_domains_and_dns_policy` в `backend/src/routing/mod.rs` (`routing::tests`: 116 passed, 0 failed)
- [DONE] Шаг 4: Бэкап на `192.168.2.1:1001` (`xr-20261114-234503-757`), применение `proxy_catalog_domains` (153 адреса переведены из `DIRECT` в `🛡️ Whitelist`, `PROXY` и `🛡️ Whitelist` сохранены на `📞 VK-туннель [БС]`), `cargo test` (738 passed, 0 failed) и `npm --prefix frontend run build` (`tsc && vite build`, built in 2.78s)
- [DONE] Шаг 5.1: Bump версии до `1.16.16` / `v1.16.16.0` (`backend/Cargo.toml`, `backend/Cargo.lock`, `frontend/package.json`), ченжлог в `DEVELOPMENT.md`, `cargo check` и `npm --prefix frontend run build` (`built in 2.15s`)
- [IN_PROGRESS] Шаг 5.2: Коммит `v1.16.16.0`, тег `v1.16.16.0`, push в `origin main`, ожидание GitHub Release CI
- [PENDING] Шаг 5.3: Установка `v1.16.16.0` на `192.168.2.1:1001` (`POST /api/backups`, `POST /api/update/install`), очистка временного `proxy_catalog_domains`, проверка каталога (249) и активного туннеля (`📞 VK-туннель [БС]`)
