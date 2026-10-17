# Project: XKeen Route Comprehensive Bug Fixes

## Architecture
- **Backend (Rust / Axum / Tokio)**:
  - `updater.rs`: Release updates, download management, SHA-256 verification, local proxy fallback.
  - `auth.rs`: Session token management, origin validation, strict IP parsing.
  - `notifications.rs`: Webhook dispatch, strict SSRF validation blocking private/loopback/link-local IP addresses.
  - `adguard.rs`: AdGuard Home integration, init scripts, configuration YAML generation, single authoritative iptables redirection (`53 -> 5353` on `br+`/`Bridge+`).
  - `speedtest.rs`: Network speed testing with RAII Drop guard ensuring node group restoration upon future cancellation.
  - `system.rs`: System operations (non-blocking async swap creation with tokio::process::Command, process management with daemon whitelist).
  - `watchdog.rs`: Periodic daemon health checks, DHCP lease synchronization, automatic AdGuard iptables enforcement and Zapret rule cleanup.
  - `failover.rs` & `traffic.rs`: Multi-node health monitoring with 3-consecutive-failure hysteresis, accurate WAN interface discovery.
  - `config.rs` & `bot.rs`: Configuration parsing, URL resolution without `0.0.0.0`, dynamic init script paths.
  - `api.rs`: REST API endpoints, routing rules, Zapret installer verification.
- **Frontend (React / TypeScript / Vite / TailwindCSS)**:
  - `Gaming.tsx`: Device-specific gaming mode toggle preserving `device_mac`.
  - `FlowRepairModal.tsx`: Explicit on-click repair without `useEffect` auto-triggering and mounted state protection.
  - `qrCode.ts`: ISO/IEC 18004 compliant QR generator (LSB format bits, alignment pattern coordinates, preserved dark module).
  - `AdGuard.tsx`: Single-point protection toggle without race-condition second calls.
  - `App.tsx`: Polling interval lower bound (Math.max(3, ...)), accessible Diagnostics tab.
  - `api.ts`, `nodeParser.ts`, `yamlValidator.ts`, `XboxDns.tsx`, `PoliciesMap.tsx`, `Help.tsx`: Resilient parsing, unicode base64, HTTP clipboard fallback, accurate API docs.

## Feature Inventory
| # | Feature | Description | Milestone | Source |
|---|---------|-------------|-----------|--------|
| 1 | SEC-01 | Remove ghproxy.net, direct GitHub with Mihomo proxy fallback, verify SHA-256 | M1 | Survey Spec Miner |
| 2 | SEC-02 | Strict IP parsing in CSRF Origin validation (is_private, is_loopback) | M1 | Survey Spec Miner |
| 3 | SEC-03 | SSRF prevention in notifications webhook URL validation | M1 | Survey Spec Miner |
| 4 | DNS-01 | Eliminate hardcoded port 3053; use detected AdGuard DNS port (5353) | M2 | Survey Spec Miner |
| 5 | DNS-02 | Unify all DNS iptables redirection in adguard.rs; remove duplicate api.rs rules | M2 | Survey Spec Miner |
| 6 | DNS-03 | Avoid duplicate bind_hosts in AdGuardHome.yaml (bind strictly to 0.0.0.0) | M2 | Survey Spec Miner |
| 7 | NET-01 | RAII Drop guard for speedtest node group switching snapshot restoration | M2 | Survey Spec Miner |
| 8 | SYS-01 | Async dd swap allocation via tokio::process::Command to prevent Tokio blocking | M2 | Survey Spec Miner |
| 9 | R3 | Zero-Touch AdGuard Home DNS Synchronization (iptables 53->5353, config.json sync, watchdog enforcement) | M2 | Survey Spec Miner |
| 10 | INS-01 | Zapret 2 ELF magic header validation (7f454c46) | M3 | Survey Explorer 2 |
| 11 | INS-02 | Reject unsupported nfqws fallback to nfqws2 | M3 | Survey Explorer 2 |
| 12 | VER-01 | Dynamic Zapret release version resolution instead of hardcoded strings | M3 | Survey Explorer 2 |
| 13 | INS-03 | Add full PATH export to S99adguardhome init script | M3 | Survey Explorer 2 |
| 14 | SYS-02 | Allow killing background daemons with PPID 1 via process whitelist | M3 | Survey Explorer 2 |
| 15 | WD-01 | Reapply Mihomo rules and cleanup DIRECT rules when Zapret halted by watchdog | M3 | Survey Explorer 2 |
| 16 | WD-02 | Preserve force_domains and device routing during DHCP lease updates | M3 | Survey Explorer 2 |
| 17 | NET-02 | Antigravity proxy bind configuration for LAN compatibility | M3 | Survey Explorer 2 |
| 18 | BOT-01 | Telegram bot uses configured zapret_init path instead of hardcode | M3 | Survey Explorer 2 |
| 19 | TR-01 | Dynamic WAN interface discovery in traffic.rs without apcli1 hardcode | M3 | Survey Explorer 2 |
| 20 | FO-01 | Failover hysteresis with 3-consecutive-failure counter | M3 | Survey Explorer 2 |
| 21 | FO-02 | Filter pseudo-nodes (DIRECT, REJECT, GLOBAL) from failover priority chain | M3 | Survey Explorer 2 |
| 22 | CF-01 | URL resolution in config.rs replacing 0.0.0.0 with 127.0.0.1 | M3 | Survey Explorer 2 |
| 23 | AG-01 | Symmetrical FORWARD chain cleanup and remove invalid ip6tables -t nat | M3 | Survey Explorer 2 |
| 24 | GAMING-01 | Per-device gaming mode toggle preserving device_mac on disable | M4 | Survey Explorer 3 |
| 25 | REPAIR-01 | Remove auto-execution from useEffect in FlowRepairModal (button only) | M4 | Survey Explorer 3 |
| 26 | REPAIR-02 | Add isMounted guard in FlowRepairModal | M4 | Survey Explorer 3 |
| 27 | QR-01 | ISO/IEC 18004 QR code generation compliance (alignment, format bits, dark module) | M4 | Survey Explorer 3 |
| 28 | FE-ADBLOCK-01 | Remove duplicate apiPost('adblock/toggle') in AdGuard.tsx | M4 | Survey Explorer 3 |
| 29 | DIAG-01 | Enable Diagnostics tab in TABS and remove hash redirect in App.tsx | M4 | Survey Explorer 3 |
| 30 | POLL-01 | Clamp refresh_interval_sec with Math.max(3, ...) to prevent DoS | M4 | Survey Explorer 3 |
| 31 | API-01 | Permit legitimate null data payloads in validateEndpointData | M4 | Survey Explorer 3 |
| 32 | POLICIES-01 | Support both is_main and is_default badge rendering in PoliciesMap | M4 | Survey Explorer 3 |
| 33 | HELP-01 | Correct API documentation endpoints in Help.tsx | M4 | Survey Explorer 3 |
| 34 | HEADER-01 | Disable core toggle button in Header.tsx until status is loaded | M4 | Survey Explorer 3 |
| 35 | ANTIGRAV-01 | Invalidate cached googleGeo on server switch | M4 | Survey Explorer 3 |
| 36 | ZAPVER-01 | Dynamic version resolution for Zapret events | M4 | Survey Explorer 3 |
| 37 | SS-01 | Unicode-safe base64 encoding in nodeParser.ts | M4 | Survey Explorer 3 |
| 38 | CLIPBOARD-01 | HTTP fallback for clipboard operations in XboxDns.tsx | M4 | Survey Explorer 3 |
| 39 | YAML-01 | Strip leading dashes before YAML key validation in yamlValidator.ts | M4 | Survey Explorer 3 |
| 40 | R4 | Anti-Regression automated tests for 100% of bug fixes | M5 | User Rule & Testing Track |

## Milestones
| # | Name | Scope | Dependencies | Status |
|---|------|-------|-------------|--------|
| 1 | M1: Security Hardening | SEC-01, SEC-02, SEC-03 | None | PLANNED |
| 2 | M2: DNS & Network Core | DNS-01, DNS-02, DNS-03, NET-01, SYS-01, R3 Zero-Touch | M1 | PLANNED |
| 3 | M3: Installer & Runtime | INS-01, INS-02, VER-01, INS-03, SYS-02, WD-01, WD-02, NET-02, BOT-01, TR-01, FO-01, FO-02, CF-01, AG-01 | M2 | PLANNED |
| 4 | M4: Frontend UX & Utils | GAMING-01, REPAIR-01/02, QR-01, FE-ADBLOCK-01, DIAG-01, POLL-01, API-01, POLICIES-01, HELP-01, HEADER-01, ANTIGRAV-01, ZAPVER-01, SS-01, CLIPBOARD-01, YAML-01 | M2, M3 | PLANNED |
| 5 | M5: Anti-Regression Verification | 100% Cargo & NPM tests passing, Forensic Audit, Zero Regressions | M1..M4 | PLANNED |

## Interface Contracts
### `updater.rs` ↔ `config.rs`
- `download_file_verified(url, sha256_expected, proxy_fallback)`: downloads file, checks sha256 if provided, falls back to `cfg.mihomo_proxy_url()`.
### `auth.rs`
- `is_valid_origin(origin_host_only)`: parses via `std::net::IpAddr`. Returns true if private/loopback IPv4, loopback IPv6, or domain is `localhost`, `my.keenetic.net`, or `router`.
### `notifications.rs`
- `validate_webhook_url(url)`: rejects if parsed IP is loopback, link-local, or private.
### `adguard.rs` ↔ `api.rs` / `watchdog.rs`
- `apply_adguard_iptables(dns_port)`: configures `XKEEN_ADGUARD` chain redirecting port 53 to `dns_port` (default 5353) on `br+` and `Bridge+`.
- `remove_adguard_iptables()`: flushes and removes `XKEEN_ADGUARD` chain from `PREROUTING`.
- All DNS redirection logic is centralized in `adguard.rs`. `api.rs` calls `adguard::apply_adguard_iptables` / `adguard::remove_adguard_iptables` instead of injecting raw rules.
### `speedtest.rs`
- `NodeGroupGuard`: RAII guard holding cloned `original_snapshot` and Arc reference, on `Drop` executes `tokio::spawn(restore_groups_snapshot(...))`.
### `Gaming.tsx` ↔ `api.rs`
- Disable gaming mode payload: `{ enabled: false, device_mac: preferredMac, mode: currentMode }`.

## Code Layout
- `backend/src/`: Backend Rust codebase
- `backend/tests/`: Integration and anti-regression tests
- `frontend/src/`: Frontend React/TypeScript codebase
- `frontend/src/tests/` and `frontend/src/utils/__tests__/`: Frontend unit and component tests
