# E2E Test Infra: XKeen Route Comprehensive Bug Fixes

## Test Philosophy
- Requirement-driven & Anti-Regression: Every single bug fix must have automated tests that fail on old code and pass on fixed code.
- Verification Commands:
  - Backend: `cargo test` in `backend/`
  - Frontend: `npm test` and `npm run build` in `frontend/`

## Feature Test Coverage Matrix
| Feature | Target Area | Test Location | Test Type |
|---------|-------------|---------------|-----------|
| SEC-01 | updater.rs / api.rs | backend/tests/sec_updater_tests.rs | Integration (SHA-256 validation & proxy fallback) |
| SEC-02 | auth.rs | backend/src/auth.rs | Unit tests (is_valid_origin strict IP matching) |
| SEC-03 | notifications.rs | backend/src/notifications.rs | Unit tests (validate_webhook_url private IP rejection) |
| DNS-01 | adguard.rs | backend/tests/dns_adguard_tests.rs | Integration (no 3053 hardcode, port 5353 redirection) |
| DNS-02 | adguard.rs / api.rs | backend/tests/dns_adguard_tests.rs | Integration (unified iptables rules, no duplicate PREROUTING) |
| DNS-03 | adguard.rs | backend/tests/dns_adguard_tests.rs | Unit/Integration (clean bind_hosts in YAML) |
| NET-01 | speedtest.rs | backend/tests/net_speedtest_tests.rs | Unit tests (Drop guard snapshot restoration on cancel) |
| SYS-01 | system.rs | backend/src/system.rs | Unit tests (async swap allocation) |
| R3 | adguard.rs / watchdog.rs | backend/tests/r3_zerotouch_tests.rs | Integration (auto-redirect 53->5353, config.json sync) |
| INS-01/02 | api.rs | backend/tests/ins_zapret_tests.rs | Unit tests (ELF magic check 7f454c46, no nfqws fallback) |
| VER-01 | api.rs | backend/tests/ins_zapret_tests.rs | Unit tests (dynamic version parsing) |
| INS-03 | adguard.rs | backend/tests/dns_adguard_tests.rs | Unit tests (S99adguardhome PATH export) |
| SYS-02 | system.rs | backend/src/system.rs | Unit tests (daemon killing with PPID 1 whitelist) |
| WD-01 | watchdog.rs | backend/tests/watchdog_tests.rs | Unit tests (Mihomo rule rebuild on Zapret disable) |
| WD-02 | watchdog.rs | backend/tests/watchdog_tests.rs | Unit tests (preservation of force_domains during lease update) |
| TR-01 | traffic.rs | backend/src/traffic.rs | Unit tests (WAN interface detection) |
| FO-01/02 | failover.rs | backend/src/failover.rs | Unit tests (hysteresis counter & pseudo-node filtering) |
| CF-01 | config.rs | backend/src/config.rs | Unit tests (0.0.0.0 URL sanitization) |
| AG-01 | api.rs | backend/tests/routing_tests.rs | Unit tests (FORWARD cleanup & ip6tables safety) |
| GAMING-01 | Gaming.tsx | frontend/src/components/__tests__/Gaming.test.tsx | Unit/Component test (payload contains device_mac when val=false) |
| REPAIR-01 | FlowRepairModal.tsx | frontend/src/components/__tests__/FlowRepairModal.test.tsx | Component test (no execution on render, only on button click) |
| QR-01 | qrCode.ts | frontend/src/utils/__tests__/qrCodeIso.test.ts | Unit tests (ISO 18004 alignment coords, format bits LSB, dark module) |
| FE-ADBLOCK-01 | AdGuard.tsx | frontend/src/components/__tests__/AdGuard.test.tsx | Component test (single API post on toggle) |
| DIAG-01 / POLL-01 | App.tsx | frontend/src/__tests__/AppNav.test.tsx | Unit/Component test (diagnostics tab included, poll interval clamped) |
| API-01 | api.ts | frontend/src/__tests__/apiValidation.test.ts | Unit test (null data handling) |
| SS-01 | nodeParser.ts | frontend/src/utils/__tests__/nodeParserUnicode.test.ts | Unit test (Unicode base64 safety) |
| YAML-01 | yamlValidator.ts | frontend/src/utils/__tests__/yamlValidator.test.ts | Unit test (list items colon validation) |

## Quality Gates
- Gate 1: 100% of Rust tests pass via `cargo test`.
- Gate 2: 100% of Frontend tests pass via `npm test` and build passes via `npm run build`.
- Gate 3: No occurrences of `--to-ports 3053` anywhere in the codebase.
- Gate 4: Zero occurrences of unverified `ghproxy.net` in binary down loaders.
- Gate 5: Forensic Integrity Audit is CLEAN.
