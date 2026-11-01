# Project: XKeen Route Failover 2.0 & Smart Failover Modernization

## Architecture
- **Backend (Rust / Axum / Tokio / Mihomo)**:
  - `backend/src/failover.rs`:
    - Core failover engine with connection delta tracking (`{id -> (upload, download)}`).
    - Blackhole anomaly detector (>5 KB upload delta, 0 download delta across >=3 sockets).
    - 4-tier fallback selection hierarchy (within threshold -> alive slow -> quarantine fast -> quarantine slow).
    - Unified `select_failover_target` engine utilized across `run_check`, Best-Ping, and `run_device_check`.
    - Server tier separation: Tier 1 (standard VPS) vs Tier 2 (whitelist servers identified by `is_whitelist_server_full`).
    - Shortened 60s Canary Soak Test for returning from Tier 2 to Tier 1 with socket reset.
    - Best-Ping engine: polling active node only during normal operation, runtime `best_ping_hysteresis_ms` enforcement, concurrency clamp expanded to 6 (default 4).
    - Real Canary payload download via local listener `http://127.0.0.1:7894` without fake constants (`8 КБ`, `15 кбит/с`).
    - Persistent quarantine stored atomically in `/opt/etc/xkeen-route/quarantine.json` (Unix timestamps).
  - `backend/src/mihomo.rs`:
    - Parallel L7 split probing (`tokio::select!` between Cloudflare & Google).
    - Elimination of redundant PROXY group retries on single proxy delay timeout.
    - Fast Retry capped at 1 retry after 1s.
    - 3-second TTL cache for `get_proxies` to eliminate redundant JSON parsing per check cycle.
  - `backend/src/routing.rs`:
    - Inbound listener configuration: `listeners: [{port: 7894, listen: 127.0.0.1, proxy: '🛡️ Canary Probe'}]`.
    - Proxy group generator: `'🛡️ Canary Probe'` isolated selector group for zero-touch canary testing.
  - `backend/src/config.rs`:
    - `smart_failover_enabled: bool` (default `true`) in `FailoverConfig`.
  - `backend/src/traffic.rs`:
    - Honest traffic accounting per channel (Direct, Proxy, Whitelist) without channel substitution.
  - `backend/src/api.rs`:
    - Status reporting for smart failover, active tier, honest BS traffic delta, and settings persistence.
- **Frontend (React / TypeScript / Vite / TailwindCSS)**:
  - `frontend/src/components/Dashboard.tsx`:
    - Failover card modernized with 10-point audit compliance (Zero Fake Metrics):
      - Purged <= 5 score elements (duplicate status badge, generic reserve count, fake `(10 мин)` string).
      - Added «🧠 Умный фейловер» toggle.
      - Added Tier 1 (VPS) / Tier 2 (БС) visual badges.
      - Displayed real Whitelist episode traffic volume from `/connections` delta.
  - `frontend/src/components/Settings.tsx`:
    - «🧠 Умный фейловер» master switch with description, synchronized with `config.failover.smart_failover_enabled`.
  - `frontend/src/types.ts`:
    - TypeScript definitions for `smart_failover_enabled`, tier status, and traffic metrics.

## Feature Inventory
| # | Feature | Description | Milestone | Source |
|---|---------|-------------|-----------|--------|
| 1 | FO-DELT-01 | Single `/connections` query per cycle with persistent snapshot `{id -> (upload, download)}` | M1 | Survey Explorer 1 |
| 2 | FO-DELT-02 | Traffic delta download > 1 KB considered live; replaces cumulative counters in `check_active_download_traffic` | M1 | Survey Explorer 1 |
| 3 | FO-HOLE-01 | Blackhole anomaly detection: upload delta > 5 KB with download delta == 0 on >= 3 active sockets | M1 | Survey Explorer 1 |
| 4 | FO-FALL-01 | 4-tier fallback hierarchy: within threshold -> alive slow -> quarantine fast -> quarantine slow, preventing blackouts | M1 | Survey Explorer 1 |
| 5 | FO-L7-01 | Concurrent L7 split probing in `ping_server_l7_split` (Cloudflare & Google in parallel, first successful wins) | M1 | Survey Explorer 1 |
| 6 | FO-L7-02 | Eliminate redundant PROXY group re-tests in `ping_server_url_expected` | M1 | Survey Explorer 1 |
| 7 | FO-L7-03 | Fast Retry capped at 1 retry after 1s; active check interval 10-15s vs pool interval 60s | M1 | Survey Explorer 1 |
| 8 | FO-SWT-01 | Eliminate 30s `LAST_SWITCH` blind delay: post-switch verification at 5s, immediate quarantine if dead | M1 | Survey Explorer 1 |
| 9 | FO-SEL-01 | Unified `select_failover_target` engine across `run_check`, Best-Ping, and `run_device_check` with quarantine bypass audit logging | M1 | Survey Explorer 1 |
| 10 | FO-TIER-01 | Server classification into Tier 1 (VPS) vs Tier 2 (Whitelist via `is_whitelist_server_full`) | M2 | Survey Explorer 2 |
| 11 | FO-TIER-02 | Tier subordination: switch to Tier 2 only if ALL Tier 1 nodes fail | M2 | Survey Explorer 2 |
| 12 | FO-TIER-03 | Accelerated 60s Canary Soak Test for returning from Tier 2 to Tier 1 with clean socket reset | M2 | Survey Explorer 2 |
| 13 | FO-PING-01 | Best-Ping optimization: poll ONLY active server when healthy, avoiding full pool ping storms | M2 | Survey Explorer 2 |
| 14 | FO-PING-02 | Runtime enforcement of `best_ping_hysteresis_ms` in candidate comparison to eliminate flapping | M2 | Survey Explorer 2 |
| 15 | FO-PING-03 | Expand `ping_servers_staggered` concurrency clamp from 2 to 6 (default 4) | M2 | Survey Explorer 2 |
| 16 | FO-CFG-01 | Add `smart_failover_enabled: bool` (default true) to `FailoverConfig`, `/api/status`, and `PUT /api/settings` | M2 | Survey Explorer 2 |
| 17 | FO-TRF-01 | Eliminate channel substitution in `traffic.rs`; track honest Whitelist episode traffic via `/connections` delta | M2 | Survey Explorer 2 |
| 18 | FO-TRN-01 | Zero-touch coordination with `handle_whitelist_transition` and gaming mode suspension/resumption | M2 | Survey Explorer 2 |
| 19 | FO-CNR-01 | Generate `'🛡️ Canary Probe'` group and listener `127.0.0.1:7894` in Mihomo configuration via `routing.rs` | M3 | Survey Explorer 3 |
| 20 | FO-CNR-02 | Real 64 KB payload download via `http://127.0.0.1:7894` with status 200, length check, and throughput calculation | M3 | Survey Explorer 3 |
| 21 | FO-CNR-03 | Eliminate all fake constants (`8 КБ`, `15 кбит/с`) from failover logs and UI | M3 | Survey Explorer 3 |
| 22 | FO-MHO-01 | 3-second TTL cache for `get_proxies` in `backend/src/mihomo.rs` to prevent repeated JSON deserialization | M3 | Survey Explorer 3 |
| 23 | FO-DEV-01 | Unify `run_device_check` with `select_failover_target` and delta download traffic verification | M4 | Survey Explorer 3 |
| 24 | FO-QUR-01 | Atomic persistence of quarantine in `/opt/etc/xkeen-route/quarantine.json` with Unix timestamps | M4 | Survey Explorer 3 |
| 25 | FO-UI-01 | Failover card 10-point audit: purge <= 5 score elements (duplicate badge, generic counter, fake `(10 мин)`) | M4 | Survey Explorer 3 |
| 26 | FO-UI-02 | Add «🧠 Умный фейловер» toggle in Dashboard Failover card and Settings tab | M4 | Survey Explorer 3 |
| 27 | FO-UI-03 | Add Tier 1 (VPS) / Tier 2 (БС) badges and honest BS traffic metric to Failover card | M4 | Survey Explorer 3 |
| 28 | FO-TST-01 | Comprehensive anti-regression unit and integration tests for all 27 features | M5 | Testing & Audit Track |

## Milestones
| # | Name | Scope | Dependencies | Status |
|---|------|-------|-------------|--------|
| 1 | M1: Traffic Delta & Core Failover | FO-DELT-01, FO-DELT-02, FO-HOLE-01, FO-FALL-01, FO-L7-01, FO-L7-02, FO-L7-03, FO-SWT-01, FO-SEL-01 | None | DONE |
| 2 | M2: Smart Failover Tiers & Best-Ping | FO-TIER-01, FO-TIER-02, FO-TIER-03, FO-PING-01, FO-PING-02, FO-PING-03, FO-CFG-01, FO-TRF-01, FO-TRN-01 | M1 | DONE |
| 3 | M3: Honest Canary Soak Test & Probe Listener | FO-CNR-01, FO-CNR-02, FO-CNR-03, FO-MHO-01 | M1, M2 | DONE |
| 4 | M4: Per-Device & Persistent Quarantine & UI Audit | FO-DEV-01, FO-QUR-01, FO-UI-01, FO-UI-02, FO-UI-03 | M2, M3 | DONE |
| 5 | M5: E2E Verification & Forensic Audit | FO-TST-01 (100% passing tests, zero fake metrics, screenshot & transcript audit) | M1..M4 | DONE |

## Interface Contracts
### `failover.rs` ↔ `mihomo.rs`
- `fetch_connections_snapshot(http, cfg) -> Result<HashMap<String, (u64, u64)>>`: captures connection byte snapshot from single `/connections` call.
- `calc_traffic_deltas(prev, current) -> (u64, u64)`: returns `(download_delta, upload_delta)`.
- `ping_server_l7_split(http, cfg, server_id, timeout_ms)`: concurrently races Cloudflare & Google expected-204 probes; returns fastest response time or -1.
- `select_failover_target(candidates, pings, is_quarantined_fn, threshold, active_id, hysteresis_ms)`: centralized pure selection engine returning `SelectedTarget { id, ping_ms, is_quarantined, is_alive_slow, tier }`.
- `is_whitelist_server_full(node_name, node_id, provider, cfg)`: authoritative server tier classifier.

### `failover.rs` ↔ `routing.rs`
- `apply_canary_rules(config_yaml)`: injects `'🛡️ Canary Probe'` proxy group and listener `127.0.0.1:7894` targeting `'🛡️ Canary Probe'`.
- `perform_canary_download(port, chunk_bytes)`: downloads chunk via `http://127.0.0.1:port` and validates status 200, length, and speed.

### `failover.rs` ↔ Storage
- `load_quarantine_from_file(path)`: loads Unix timestamp entries from `/opt/etc/xkeen-route/quarantine.json`.
- `save_quarantine_to_file(path, entries)`: atomically writes quarantine records via temp file.

## Code Layout
- `backend/src/failover.rs`: Core failover engine, tier subordination, selection, canary test
- `backend/src/mihomo.rs`: Mihomo REST client, L7 probing, TTL cache
- `backend/src/routing.rs`: YAML rule and listener injection
- `backend/src/config.rs`: `FailoverConfig` definitions
- `backend/src/traffic.rs`: Real-time traffic measurement
- `backend/src/api.rs`: REST API endpoints and settings
- `frontend/src/components/Dashboard.tsx`: Failover card UI
- `frontend/src/components/Settings.tsx`: Settings UI
