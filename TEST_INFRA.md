# E2E Test Infra: Failover 2.0 & Smart Failover

## Test Philosophy
- Opaque-box, requirement-driven verification derived from ORIGINAL_REQUEST.md.
- Methodology: Category-Partition + Boundary Value Analysis + Pairwise Interaction Testing + Real-World Workload Testing.
- 100% green tests in `cargo test` and `npm test` / `npm run build`.
- Zero tolerance for fake metrics, hardcoded bypasses, or skipped checks.

## Feature Inventory & Test Coverage Goals
| # | Feature | Requirement | Tier 1 (Coverage) | Tier 2 (Boundary) | Tier 3 (Pairwise) | Tier 4 (Scenario) |
|---|---------|-------------|:-----------------:|:-----------------:|:-----------------:|:-----------------:|
| 1 | FO-DELT-01..02 | Traffic Delta Accounting | 5 tests | 5 tests | ✓ | ✓ |
| 2 | FO-HOLE-01 | Blackhole Anomaly Detection | 5 tests | 5 tests | ✓ | ✓ |
| 3 | FO-FALL-01 | 4-Tier Fallback Hierarchy | 5 tests | 5 tests | ✓ | ✓ |
| 4 | FO-L7-01..03 | Parallel L7 Split & Fast Retry | 5 tests | 5 tests | ✓ | ✓ |
| 5 | FO-SWT-01 | 5s Post-Switch Fast Recheck | 5 tests | 5 tests | ✓ | ✓ |
| 6 | FO-SEL-01 | Unified Target Selection | 5 tests | 5 tests | ✓ | ✓ |
| 7 | FO-TIER-01..03 | Smart Failover Tiers (1 vs 2) | 5 tests | 5 tests | ✓ | ✓ |
| 8 | FO-PING-01..03 | Best-Ping Hysteresis & Isolation | 5 tests | 5 tests | ✓ | ✓ |
| 9 | FO-CFG-01 | Smart Failover Config Toggle | 5 tests | 5 tests | ✓ | ✓ |
| 10 | FO-TRF-01 | Honest Whitelist Traffic Delta | 5 tests | 5 tests | ✓ | ✓ |
| 11 | FO-CNR-01..03 | Real Canary Soak Test & Listener | 5 tests | 5 tests | ✓ | ✓ |
| 12 | FO-MHO-01 | Mihomo Proxies TTL Cache | 5 tests | 5 tests | ✓ | ✓ |
| 13 | FO-DEV-01 | Unified Per-Device Failover | 5 tests | 5 tests | ✓ | ✓ |
| 14 | FO-QUR-01 | Atomic Quarantine Persistence | 5 tests | 5 tests | ✓ | ✓ |
| 15 | FO-UI-01..03 | UI Zero Fake Metrics & Badges | 5 tests | 5 tests | ✓ | ✓ |

## Test Architecture
- Backend Unit & Integration Tests: `backend/tests/` and `backend/src/failover.rs` `#[cfg(test)]`.
- Frontend Unit & Visual Tests: `frontend/src/tests/` and Vitest component tests.
- Verification semantic: 0 exit code, 0 compilation warnings, strict assertions.
