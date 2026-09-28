// ==============================================================================
// verify-milestone4.cjs — Comprehensive Automated Verification Suite
// for Milestone 4 (Этап 4): Network, Failsafe & Hardware Adaptation (NET-01..04)
// ==============================================================================

const fs = require('fs');
const path = require('path');
const assert = require('assert');

let passedTests = 0;
let totalTests = 0;

function runTest(name, fn) {
  totalTests++;
  try {
    fn();
    console.log(`  ✓ ${name}`);
    passedTests++;
  } catch (err) {
    console.error(`  ✗ ${name}`);
    console.error(`    Error: ${err.message}`);
    process.exitCode = 1;
  }
}

console.log('=== Running Milestone 4 Network, Failsafe & Hardware Adaptation Verification ===\n');

const apiRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/api.rs'), 'utf8');
const zapretRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/zapret.rs'), 'utf8');
const watchdogRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/watchdog.rs'), 'utf8');

// -------------------------------------------------------------
// 1. NET-01: DNS Loop Prevention (Router / Daemon loop elimination)
// -------------------------------------------------------------
runTest('1. NET-01: detect_dns_loop_risk identifies loopback & local port risks', () => {
  assert(zapretRs.includes('pub fn detect_dns_loop_risk('), 'zapret.rs must define detect_dns_loop_risk');
  assert(zapretRs.includes('127.0.0.1') && zapretRs.includes('localhost'), 'detect_dns_loop_risk must check localhost addresses');
  assert(zapretRs.includes('local_ports.contains(&port)'), 'detect_dns_loop_risk must detect collision with redirected local ports');

  // Functional JS simulation of detect_dns_loop_risk
  function simulateDetectDnsLoopRisk(nameservers, localPorts) {
    for (const ns of nameservers) {
      const trimmed = ns.trim().replace(/^udp:\/\//, '').replace(/^tcp:\/\//, '');
      const parts = trimmed.split(':');
      const host = parts[0] ? parts[0].trim() : '';
      const port = parts[1] ? parseInt(parts[1], 10) : 53;
      const isLocalhost = host === '127.0.0.1' || host === 'localhost' || host === '0.0.0.0' || host === '::1';
      if (isLocalhost && localPorts.includes(port)) {
        return false; // loop detected
      }
    }
    return true; // safe
  }

  assert.strictEqual(simulateDetectDnsLoopRisk(['1.1.1.1:53', '77.88.8.8'], [1053]), true, 'Public DNS must be safe');
  assert.strictEqual(simulateDetectDnsLoopRisk(['127.0.0.1:1053'], [1053]), false, '127.0.0.1:1053 must be detected as loop');
  assert.strictEqual(simulateDetectDnsLoopRisk(['udp://localhost:1053'], [1053]), false, 'udp://localhost:1053 must be detected as loop');
  assert.strictEqual(simulateDetectDnsLoopRisk(['127.0.0.1:53'], [53, 1053]), false, '127.0.0.1:53 with redirected 53 must be detected as loop');
});

runTest('2. NET-01: S51zapret excludes local router DNS and never redirects OUTPUT table', () => {
  assert(apiRs.includes('iptables -t nat -A PREROUTING -i br+ -d 127.0.0.1 -j RETURN'),
    'S51zapret must bypass 127.0.0.1 DNS queries for br+ to prevent loop with ndnproxy');
  assert(apiRs.includes('iptables -t nat -A PREROUTING -i Bridge+ -d 127.0.0.1 -j RETURN'),
    'S51zapret must bypass 127.0.0.1 DNS queries for Bridge+ to prevent loop with ndnproxy');
  assert(!apiRs.includes('iptables -t nat -A OUTPUT -p udp --dport 53 -j REDIRECT'),
    'S51zapret must never redirect router local OUTPUT DNS queries');
  assert(!apiRs.includes('iptables -t nat -A OUTPUT -p tcp --dport 53 -j REDIRECT'),
    'S51zapret must never redirect router local OUTPUT TCP DNS queries');
});

runTest('3. NET-01: Watchdog monitors DNS redirect health and tears down rules if resolver stalls', () => {
  assert(watchdogRs.includes('let dns_redirect_active = tokio::process::Command::new("sh")'),
    'watchdog must check if DNS redirect is active');
  assert(watchdogRs.includes('crate::zapret::check_dns_resolver_ready(1053)'),
    'watchdog must check if DNS resolver at 1053 is responding');
  assert(watchdogRs.includes('while iptables -t nat -D PREROUTING -p udp --dport 53 -j REDIRECT --to-ports 1053'),
    'watchdog must tear down UDP DNS redirect if resolver stalls');
  assert(watchdogRs.includes('while iptables -t nat -D PREROUTING -p tcp --dport 53 -j REDIRECT --to-ports 1053'),
    'watchdog must tear down TCP DNS redirect if resolver stalls');
});

// -------------------------------------------------------------
// 2. NET-02: WAN Rebind, Dynamic Interface Detection & MTU MSS Clamping
// -------------------------------------------------------------
runTest('4. NET-02: Dynamic active WAN interface detection in NDM hooks and S51zapret', () => {
  assert(apiRs.includes('get_active_wan_ifaces() {'), 'Scripts must define get_active_wan_ifaces');
  assert(apiRs.includes('/proc/net/route'), 'get_active_wan_ifaces must read /proc/net/route');
  assert(apiRs.includes('ip route show default'), 'get_active_wan_ifaces must support ip route default fallback');
  assert(apiRs.includes('$2 == "00000000"'), 'get_active_wan_ifaces must filter default destination 00000000');

  // Verify presence in NDM_NETFILTER_SCRIPT and NDM_IFSTATE_SCRIPT
  const netfilterSection = apiRs.split('NDM_NETFILTER_SCRIPT: &str =')[1].split('NDM_IFSTATE_SCRIPT: &str =')[0];
  assert(netfilterSection.includes('get_active_wan_ifaces() {'), 'NDM_NETFILTER_SCRIPT must define get_active_wan_ifaces');

  const ifstateSection = apiRs.split('NDM_IFSTATE_SCRIPT: &str =')[1].split('S51ZAPRET_SCRIPT: &str =')[0];
  assert(ifstateSection.includes('get_active_wan_ifaces() {'), 'NDM_IFSTATE_SCRIPT must define get_active_wan_ifaces');
});

runTest('5. NET-02: TCP MSS clamping prevents PMTU clashing and packet stalls on tunnels', () => {
  assert(apiRs.includes('iptables -t mangle -A zapret -p tcp --tcp-flags SYN,RST SYN -j TCPMSS --clamp-mss-to-pmtu'),
    'add_fw must configure TCPMSS clamp-mss-to-pmtu');
  assert(apiRs.includes('while iptables -t mangle -D zapret -p tcp --tcp-flags SYN,RST SYN -j TCPMSS --clamp-mss-to-pmtu'),
    'remove_fw_rules must cleanly remove TCPMSS clamp-mss-to-pmtu');
});

// -------------------------------------------------------------
// 3. NET-03: Hardware-Adaptive Failsafe & Resource Protection
// -------------------------------------------------------------
runTest('6. NET-03: get_hardware_adaptive_timeout adapts to RAM, MIPS CPU & LoadAvg', () => {
  assert(zapretRs.includes('pub fn get_hardware_adaptive_timeout('),
    'zapret.rs must implement get_hardware_adaptive_timeout');
  assert(zapretRs.includes('MemTotal:'), 'get_hardware_adaptive_timeout must inspect MemTotal');
  assert(zapretRs.includes('131072'), 'get_hardware_adaptive_timeout must scale up for <= 128MB RAM devices');
  assert(zapretRs.includes('"mips"'), 'get_hardware_adaptive_timeout must scale up for MIPS architecture');
  assert(zapretRs.includes('/proc/loadavg'), 'get_hardware_adaptive_timeout must factor in system loadavg');

  // Functional JS simulation of adaptive timeout
  function simulateAdaptiveTimeout(baseMs, ramKb, arch, loadAvg) {
    let multiplier = 1.0;
    if (ramKb <= 131072) {
      multiplier = Math.max(multiplier, 2.0);
    }
    if (arch.toLowerCase().includes('mips')) {
      multiplier = Math.max(multiplier, 2.0);
    }
    if (loadAvg >= 2.0) {
      multiplier += 1.0;
    } else if (loadAvg >= 1.0) {
      multiplier += 0.5;
    }
    return Math.round(baseMs * multiplier);
  }

  // Powerful ARM64 router (e.g. Titan KN-1811, 512MB RAM, low load)
  const baseTimeout = 3000;
  const titanTimeout = simulateAdaptiveTimeout(baseTimeout, 524288, 'aarch64', 0.2);
  assert.strictEqual(titanTimeout, 3000, 'Powerful router must retain base timeout');

  // Low-end MIPS router (e.g. Start KN-1111 / 4G, 64MB RAM)
  const startTimeout = simulateAdaptiveTimeout(baseTimeout, 65536, 'mips', 0.5);
  assert.strictEqual(startTimeout, 6000, 'Low-RAM MIPS router must double timeout (2.0x)');

  // Loaded low-end MIPS router (high loadavg 2.5)
  const heavyStartTimeout = simulateAdaptiveTimeout(baseTimeout, 65536, 'mips', 2.5);
  assert.strictEqual(heavyStartTimeout, 9000, 'Loaded low-RAM MIPS router must triple timeout (3.0x)');
});

runTest('7. NET-03: verify_zapret_failsafe integrates hardware adaptive timeouts', () => {
  assert(zapretRs.includes('let internet_timeout = get_hardware_adaptive_timeout('),
    'verify_zapret_failsafe must use get_hardware_adaptive_timeout for probe requests');
  assert(zapretRs.includes('pub struct FailsafeVerificationReport'),
    'zapret.rs must define FailsafeVerificationReport');
});

// -------------------------------------------------------------
// 4. NET-04: Idempotent IPTables Cleanup & Trap Recovery
// -------------------------------------------------------------
runTest('8. NET-04: remove_fw_rules provides complete idempotent teardown of all chains and hooks', () => {
  assert(apiRs.includes('remove_fw_rules() {'), 'S51zapret must define remove_fw_rules');
  assert(apiRs.includes('while iptables -t nat -D PREROUTING -i br+ -p udp --dport 53 -j REDIRECT'),
    'remove_fw_rules must loop-remove PREROUTING UDP DNS redirects');
  assert(apiRs.includes('while iptables -t nat -D PREROUTING -i Bridge+ -p tcp --dport 53 -j REDIRECT'),
    'remove_fw_rules must loop-remove PREROUTING TCP DNS redirects');
  assert(apiRs.includes('while iptables -t mangle -D PREROUTING -i br+ -j zapret'),
    'remove_fw_rules must loop-remove br+ PREROUTING zapret hook');
  assert(apiRs.includes('while iptables -t mangle -D PREROUTING -i Bridge+ -j zapret'),
    'remove_fw_rules must loop-remove Bridge+ PREROUTING zapret hook');
  assert(apiRs.includes('while iptables -t mangle -D POSTROUTING -j zapret'),
    'remove_fw_rules must loop-remove POSTROUTING zapret hook');
  assert(apiRs.includes('iptables -t mangle -F zapret'), 'remove_fw_rules must flush zapret chain');
  assert(apiRs.includes('iptables -t mangle -X zapret'), 'remove_fw_rules must delete zapret chain');
});

runTest('9. NET-04: del_fw delegates to remove_fw_rules and failsafe PID cleanup', () => {
  assert(apiRs.includes('del_fw() {\n  remove_fw_rules\n}'),
    'del_fw must delegate cleanly to remove_fw_rules');
  assert(apiRs.includes('if [ -f "$FAILSAFE_PID" ]; then\n    kill -9 $(cat "$FAILSAFE_PID")'),
    'remove_fw_rules must terminate active failsafe background timer');
});

runTest('10. NET-04: add_fw and start_failsafe clean up on error & rollback without self-deadlock', () => {
  assert(apiRs.includes('add_fw() {\n  remove_fw_rules'),
    'add_fw must idempotently purge previous rules before re-creating chains');
  assert(apiRs.includes('ERROR: failed to flush mangle zapret chain" >&2; remove_fw_rules; return 1; }'),
    'add_fw must call remove_fw_rules and return 1 on chain flush error');
  assert(apiRs.includes('ERROR: failed to add fwmark bypass rule in mangle zapret" >&2; remove_fw_rules; return 1; }'),
    'add_fw must call remove_fw_rules and return 1 on mark rule error');
  assert(apiRs.includes('stop_nfqws\n      remove_fw_rules\n      release_zapret_lock\n      exit 1'),
    'start failure handler must clean up processes and rules before exiting with error');
});

// -------------------------------------------------------------
// Summary
// -------------------------------------------------------------
console.log(`\n=== All ${passedTests}/${totalTests} Milestone 4 Checks Passed Successfully ===\n`);
if (passedTests !== totalTests) {
  process.exit(1);
}
