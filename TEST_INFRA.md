# E2E Test Infra: YouTube Playback & VK Calls Tunnel Optimization

## Test Philosophy
- Opaque-box and requirement-driven empirical validation.
- Verification targets:
  - QUIC UDP/443 rejection (<10ms ICMP Port Unreachable).
  - TLS 1.3 Handshake over `csq0` (<2s, 0% HTTP timeouts).
  - Happy Eyeballs parallel connection dialing (`tcp-concurrent: true`).
  - Failover selector preservation on manual selection of `📞 VK-туннель [БС]`.
  - Target router 192.168.2.1 safety and backup `xr-20261113-200126-766` intactness.

## Feature Inventory
| # | Feature | Requirement | Tier 1 (Unit) | Tier 2 (Boundary) | Tier 3 (Integration) | Tier 4 (Live Router) |
|---|---------|-------------|:-------------:|:-----------------:|:--------------------:|:--------------------:|
| 1 | QUIC-ORD-01 | R1, R2 | ✓ | ✓ | ✓ | ✓ |
| 2 | QUIC-INGR-01 | R1, R2 | ✓ | ✓ | ✓ | ✓ |
| 3 | VK-PROV-01 | R1, R3 | ✓ | ✓ | ✓ | ✓ |
| 4 | VK-IPT-01 | R1, R3 | ✓ | ✓ | ✓ | ✓ |
| 5 | TRN-MTU-01 | R3 | ✓ | ✓ | ✓ | ✓ |
| 6 | TRN-WRK-01 | R3 | ✓ | ✓ | ✓ | ✓ |
| 7 | DNS-HE-01 | R4 | ✓ | ✓ | ✓ | ✓ |
| 8 | FO-SEL-01 | R4 | ✓ | ✓ | ✓ | ✓ |
| 9 | FO-SEL-02 | R4 | ✓ | ✓ | ✓ | ✓ |
| 10 | TST-REG-01 | R5 | ✓ | ✓ | ✓ | ✓ |
| 11 | VLD-RTR-01 | R5 | ✓ | ✓ | ✓ | ✓ |

## Test Commands
- Backend anti-regression unit & integration tests:
  ```powershell
  cd backend
  cargo test
  ```
- Live Router verification:
  - Verify QUIC port unreachable:
    ```powershell
    python -c "import socket; s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM); s.settimeout(1.0); s.connect(('142.250.74.78', 443)); s.send(b'\x00'*10); s.recv(1024)"
    ```
  - Verify TLS handshake < 2s over `csq0`:
    ```bash
    curl -v --interface csq0 --max-time 5 https://www.youtube.com/
    ```
  - Verify Mihomo `tcp-concurrent: true` and active rules.
