# Project: YouTube Playback & VK Calls Tunnel Optimization

## Architecture
- **Backend (Rust / Axum / Tokio / Mihomo)**:
  - `backend/src/routing.rs`:
    - Insertion of managed QUIC rejection rule block (`AND,((NETWORK,UDP),(DST-PORT,443)),REJECT`) strictly before `# --- AUTO-WHITELIST-BEGIN ---` and all `SRC-IP-CIDR` rules to prevent rule shadowing.
    - Generation and persistence of `tcp-concurrent: true` in Mihomo configuration to enable RFC 8305 Happy Eyeballs parallel TCP dialing.
  - `backend/src/api.rs`:
    - Kernel TPROXY ingress REJECT rule in `build_whitelist_full_tunnel_commands`: `-t filter -I INPUT 1 -m mark --mark 0x115/0x115 -p udp --dport 443 -j REJECT --reject-with icmp-port-unreachable` so client browsers receive immediate ICMP rejection.
  - `backend/src/vkturn.rs`:
    - Update `render_provider_yaml()`: set `udp: false` for `📞 VK-туннель [БС]` to prevent Mihomo from attempting QUIC over `csq0`.
    - Update `iptables_apply_script()`: persistently maintain `-t filter -I OUTPUT/FORWARD ... -p udp --dport 443 -j REJECT --reject-with icmp-port-unreachable`.
    - Update `tun_configure_script`: enable kernel TCP MTU probing (`echo 1 > /proc/sys/net/ipv4/tcp_mtu_probing`) to prevent packet blackhole stalls on multi-segment TLS ClientHello.
    - Rationalize worker thread count to reduce parallel TURN UDP jitter and packet reordering.
  - `backend/src/failover.rs`:
    - Guard `should_restore_smart` and `should_restore_chain` with `!is_manual_active` when the active server (specifically `crate::vkturn::PROXY_NAME`) is manually selected by the user.
    - Prevent Canary soak tests from switching selector groups away from the manually selected VK Calls tunnel.
- **Router Deployment & Validation (192.168.2.1)**:
  - Router SSH port 222, Web API port 1001, Mihomo API port 9090.
  - Verification of backup `xr-20261113-200126-766` integrity.
  - Zero password modifications and strict clean-up / rollback safety.

## Feature Inventory
| # | Feature | Description | Milestone | Source |
|---|---------|-------------|-----------|--------|
| 1 | QUIC-ORD-01 | Managed QUIC rejection rule placed above Auto-Whitelist and all `SRC-IP-CIDR` rules in `routing.rs` | M1 | Survey Explorer 1 |
| 2 | QUIC-INGR-01 | Kernel ingress REJECT with `icmp-port-unreachable` in `build_whitelist_full_tunnel_commands` (`api.rs`) | M1 | Survey Explorer 1 |
| 3 | VK-PROV-01 | `render_provider_yaml()` outputs `udp: false` for `📞 VK-туннель [БС]` in `vkturn.rs` | M1 | Survey Explorer 2 |
| 4 | VK-IPT-01 | `iptables_apply_script()` persistently maintains filter table UDP 443 REJECT rules in `vkturn.rs` | M1 | Survey Explorer 2 |
| 5 | TRN-MTU-01 | Kernel `tcp_mtu_probing` enabled in `tun_configure_script` (`vkturn.rs`) for blackhole detection | M1 | Survey Explorer 2 |
| 6 | TRN-WRK-01 | Rationalized worker count in `vkturn.rs` to minimize UDP packet loss and jitter | M1 | Survey Explorer 2 |
| 7 | DNS-HE-01 | `tcp-concurrent: true` generated and maintained in Mihomo config for Happy Eyeballs parallel dial | M1 | Survey Explorer 3 |
| 8 | FO-SEL-01 | `failover.rs` suppresses `should_restore_smart` and `should_restore_chain` when VK tunnel is manually selected | M1 | Survey Explorer 3 |
| 9 | FO-SEL-02 | Guard primary and secondary selector groups against Canary soak test switching while manual VK tunnel active | M1 | Survey Explorer 3 |
| 10 | TST-REG-01 | Automated unit and regression tests in `routing.rs`, `vkturn.rs`, and `failover.rs` (`cargo test`) | M1 | Survey Explorers 1, 2, 3 |
| 11 | VLD-RTR-01 | Deployment and empirical validation on router 192.168.2.1 (curl < 2s, YouTube streaming, backup intact) | M1 | Survey Explorers 1, 2, 3 |

## Milestones
| # | Name | Scope | Dependencies | Status |
|---|------|-------|-------------|--------|
| 1 | M1: YouTube VK Calls Optimization & Router Validation | QUIC-ORD-01, QUIC-INGR-01, VK-PROV-01, VK-IPT-01, TRN-MTU-01, TRN-WRK-01, DNS-HE-01, FO-SEL-01, FO-SEL-02, TST-REG-01, VLD-RTR-01 | None | IN_PROGRESS |

## Interface Contracts
### `routing.rs` ↔ `mihomo.rs` / Config
- `apply_routing_with_filters`: Emits `# --- AUTO-QUIC-REJECT-BEGIN ---` with `AND,((NETWORK,UDP),(DST-PORT,443)),REJECT` placed before `# --- AUTO-WHITELIST-BEGIN ---`.
- Config normalizer: Sets `tcp-concurrent: true` in `config.yaml`.

### `vkturn.rs` ↔ Kernel & Mihomo
- `render_provider_yaml() -> String`: Generates `udp: false` for `📞 VK-туннель [БС]`.
- `iptables_apply_script()`: Injects `-t filter -I OUTPUT/FORWARD ... -p udp --dport 443 -j REJECT --reject-with icmp-port-unreachable`.
- `tun_configure_script()`: Injects `echo 1 > /proc/sys/net/ipv4/tcp_mtu_probing`.

### `failover.rs` ↔ State
- `run_check()`: Checks `is_manual_active` and `active.id == crate::vkturn::PROXY_NAME`, bypassing `should_restore_smart`, `should_restore_chain`, and selector group reassignment.

## Code Layout
- `backend/src/routing.rs`: Mihomo routing rules generator and QUIC rule placement.
- `backend/src/api.rs`: Full tunnel iptables commands and TPROXY ingress rules.
- `backend/src/vkturn.rs`: `csqtt` daemon management, TUN configuration, provider YAML, and iptables.
- `backend/src/failover.rs`: Failover state machine, Canary Soak Test, and manual switch persistence.
