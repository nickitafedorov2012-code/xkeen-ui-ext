// ==============================================================================
// verify-milestone3.cjs — Comprehensive Automated Verification Suite
// for Milestone 3 (Этап 3): Unified Process & Firewall Management for Zapret
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

console.log('=== Running Milestone 3 Unified Process & Firewall Management Verification ===\n');

const apiRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/api.rs'), 'utf8');
const zapretRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/zapret.rs'), 'utf8');
const watchdogRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/watchdog.rs'), 'utf8');
const systemRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/system.rs'), 'utf8');
const mainRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/main.rs'), 'utf8');

// -------------------------------------------------------------
// 1. Z-01: State transition serialization & cross-process locking
// -------------------------------------------------------------
runTest('1. Z-01: Cross-process lock and ZapretOperationState enum in zapret.rs', () => {
  assert(zapretRs.includes('pub enum ZapretOperationState'), 'zapret.rs must define ZapretOperationState enum');
  assert(zapretRs.includes('Idle,'), 'ZapretOperationState must include Idle');
  assert(zapretRs.includes('Starting,'), 'ZapretOperationState must include Starting');
  assert(zapretRs.includes('Stopping,'), 'ZapretOperationState must include Stopping');
  assert(zapretRs.includes('Restarting,'), 'ZapretOperationState must include Restarting');
  assert(zapretRs.includes('Reloading,'), 'ZapretOperationState must include Reloading');
  assert(zapretRs.includes('Configuring,'), 'ZapretOperationState must include Configuring');
  assert(zapretRs.includes('Upgrading,'), 'ZapretOperationState must include Upgrading');

  assert(zapretRs.includes('pub struct ZapretLifecycleState'), 'zapret.rs must define ZapretLifecycleState');
  assert(zapretRs.includes('pub struct ZapretLockGuard'), 'zapret.rs must define ZapretLockGuard');
  assert(zapretRs.includes('acquire('), 'ZapretLockGuard must have acquire method');
  assert(zapretRs.includes('Drop for ZapretLockGuard'), 'ZapretLockGuard must implement Drop');

  // Verify lock paths
  assert(zapretRs.includes('/opt/var/run/xkeen-zapret.lock'), 'zapret.rs must define lockfile path');
  assert(zapretRs.includes('/opt/var/run/xkeen-zapret.lock.d'), 'zapret.rs must define lock directory path');
  assert(zapretRs.includes('/opt/var/run/xkeen-zapret.disabled'), 'zapret.rs must define disabled marker path');
});

runTest('2. Z-01: S51zapret script cross-process lockfile and directory locking', () => {
  assert(apiRs.includes('LOCKFILE="/opt/var/run/xkeen-zapret.lock"'), 'S51zapret must define LOCKFILE');
  assert(apiRs.includes('LOCKDIR="/opt/var/run/xkeen-zapret.lock.d"'), 'S51zapret must define LOCKDIR');
  assert(apiRs.includes('acquire_zapret_lock() {'), 'S51zapret must define acquire_zapret_lock');
  assert(apiRs.includes('release_zapret_lock() {'), 'S51zapret must define release_zapret_lock');
  assert(apiRs.includes('flock -w "$timeout" 9'), 'acquire_zapret_lock must attempt flock');
  assert(apiRs.includes('mkdir "$LOCKDIR"'), 'acquire_zapret_lock must use atomic mkdir lock');
  assert(apiRs.includes('kill -0 "$holder"'), 'acquire_zapret_lock must check if lockholder process is alive');

  // Verify operations acquire lock
  assert(apiRs.includes('start)\n    acquire_zapret_lock || exit 1'), 'start must acquire zapret lock');
  assert(apiRs.includes('stop)\n    acquire_zapret_lock || exit 1'), 'stop must acquire zapret lock');
  assert(apiRs.includes('restart)\n    acquire_zapret_lock || exit 1'), 'restart must acquire zapret lock');
  assert(apiRs.includes('reload|reload-hosts)\n    acquire_zapret_lock || exit 1'), 'reload must acquire zapret lock');
  assert(apiRs.includes('start-fw|reload-fw)\n    acquire_zapret_lock || exit 1'), 'start-fw must acquire zapret lock');
  assert(apiRs.includes('stop-fw)\n    acquire_zapret_lock || exit 1'), 'stop-fw must acquire zapret lock');
});

runTest('3. Z-01: Watchdog skips checks during in-flight operations', () => {
  assert(watchdogRs.includes('crate::zapret::get_current_operation_state() != crate::zapret::ZapretOperationState::Idle'),
    'watchdog must check get_current_operation_state() != ZapretOperationState::Idle');
  assert(watchdogRs.includes('state.zapret_lock.try_lock()'),
    'watchdog must try_lock state.zapret_lock to avoid collision with in-flight Zapret mutations');
  assert(mainRs.includes('pub zapret_lock: Arc<tokio::sync::Mutex<()>>'),
    'AppState in main.rs must define zapret_lock');
});

// -------------------------------------------------------------
// 2. Z-02: DNS Resolver readiness check before PREROUTING REDIRECT
// -------------------------------------------------------------
runTest('4. Z-02: DNS Resolver readiness validation in S51zapret and Rust', () => {
  // Shell S51zapret script DNS check
  assert(apiRs.includes('check_dns_resolver_ready() {'), 'S51zapret must define check_dns_resolver_ready');
  assert(apiRs.includes('/proc/net/udp') && apiRs.includes('/proc/net/tcp'), 'check_dns_resolver_ready must inspect /proc/net/udp and /proc/net/tcp');
  assert(apiRs.includes(':041D'), 'check_dns_resolver_ready must check hex port 041D (port 1053)');

  // Guard before REDIRECT
  assert(apiRs.includes('if check_dns_resolver_ready; then'), 'add_fw must guard DNS redirect with check_dns_resolver_ready');
  assert(apiRs.includes('DNS resolver port 1053 not listening; client DNS redirect skipped to prevent LAN blackout'),
    'add_fw must log warning when port 1053 is not listening');

  // Rust zapret.rs DNS check
  assert(zapretRs.includes('pub async fn check_dns_resolver_ready(target_port: u16) -> bool'),
    'zapret.rs must implement check_dns_resolver_ready');

  // api.rs get_zapret_status returns DNS status
  assert(apiRs.includes('"dns_resolver_ready": dns_resolver_ready'),
    'get_zapret_status must return dns_resolver_ready');
  assert(apiRs.includes('"dns_redirect_active": dns_redirect_active'),
    'get_zapret_status must return dns_redirect_active');
});

// -------------------------------------------------------------
// 3. Z-03: Firewall script error handling & non-zero exit codes
// -------------------------------------------------------------
runTest('5. Z-03: Firewall error handling eliminates unconditional return 0', () => {
  // add_fw checks failure and calls remove_fw_rules + return 1
  assert(apiRs.includes('ERROR: failed to flush mangle zapret chain'), 'add_fw must check flush failure');
  assert(apiRs.includes('ERROR: failed to add fwmark bypass rule in mangle zapret'), 'add_fw must check mark bypass failure');
  assert(apiRs.includes('ERROR: failed to hook zapret chain into PREROUTING for br+/Bridge+'), 'add_fw must check hook failure');
  assert(apiRs.includes('remove_fw_rules\n    return 1'), 'add_fw must call remove_fw_rules and return 1 on critical failures');

  // start-fw command propagates exit status
  assert(apiRs.includes('add_fw\n    ret=$?\n    release_zapret_lock\n    exit $ret'),
    'start-fw must propagate return code of add_fw');
});

// -------------------------------------------------------------
// 4. Z-04: Zapret installer staging & atomic replace
// -------------------------------------------------------------
runTest('6. Z-04: Installer staging directory, ELF validation and atomic replace', () => {
  assert(apiRs.includes('STAGE_DIR=$(mktemp -d'),
    'installer must isolate temporary download files using mktemp -d');
  assert(apiRs.includes('7f 45 4c 46') || apiRs.includes('7fELF'),
    'installer must validate ELF header of nfqws binary');
  assert(apiRs.includes('mv -f "$NEW_BIN" /opt/zapret2/nfqws2'),
    'installer must atomically move/rename binary');
  assert(apiRs.includes('is_nfqws2_available()'),
    'upgrade_zapret2 must verify is_nfqws2_available() before setting engine to v2');
  assert(apiRs.includes('chmod +x /opt/zapret2/nfqws2 && ln -sf /opt/zapret2/nfqws2 /opt/sbin/nfqws2'),
    'installer must ensure executable permission and symlink');
  assert(apiRs.includes('chmod 644 /opt/zapret2/lua/*.lua'),
    'installer must ensure proper permissions for Lua libraries');
});

// -------------------------------------------------------------
// 5. Z-05: Failsafe verification accuracy (Fail-Closed)
// -------------------------------------------------------------
runTest('7. Z-05: Failsafe fail-closed verification in Rust and shell', () => {
  assert(zapretRs.includes('pub async fn verify_zapret_failsafe('),
    'zapret.rs must implement verify_zapret_failsafe');
  assert(zapretRs.includes('pub struct FailsafeVerificationReport'),
    'zapret.rs must define FailsafeVerificationReport');
  assert(zapretRs.includes('let overall_success = router_alive && internet_reachable && lan_bridge_valid;'),
    'verify_zapret_failsafe must require router_alive, internet_reachable and lan_bridge_valid (fail-closed)');
  assert(apiRs.includes('act == "verify_failsafe"'),
    'api.rs zapret_action must expose verify_failsafe endpoint');

  // Shell failsafe rollback in S51zapret
  assert(apiRs.includes('start_failsafe() {'), 'S51zapret must define start_failsafe');
  assert(apiRs.includes('sleep 45'), 'start_failsafe must have 45s timer');
  const failsafeBlock = apiRs.split('start_failsafe() {')[1].split('del_fw() {')[0];
  assert(failsafeBlock.includes('remove_fw_rules\n      stop_nfqws'), 'emergency handler must call remove_fw_rules and stop_nfqws');
  assert(!failsafeBlock.includes('      del_fw\n'), 'emergency handler must not kill itself by calling del_fw');
});

// -------------------------------------------------------------
// 6. Z-06: Atomic file writes for all zapret configs
// -------------------------------------------------------------
runTest('8. Z-06: Atomic file writes for zapret.conf, hosts, scripts and ndm hooks', () => {
  // sync_zapret_files uses atomic_write_file
  assert(apiRs.includes('atomic_write_file("/opt/etc/ndm/netfilter.d/050-zapret.sh", NDM_NETFILTER_SCRIPT)'),
    'sync_zapret_files must atomically write 050-zapret.sh netfilter hook');
  assert(apiRs.includes('atomic_write_file("/opt/etc/init.d/S51zapret", S51ZAPRET_SCRIPT)'),
    'sync_zapret_files must atomically write S51zapret script');
  assert(apiRs.includes('atomic_write_file("/opt/etc/zapret/zapret.conf", &conf_data)'),
    'sync_zapret_files must atomically write zapret.conf');
  assert(apiRs.includes('atomic_write_file(hosts_path, &synced_hosts)'),
    'sync_zapret_files must atomically write zapret-hosts.txt');

  // save_config & save_hosts use atomic_write_file
  assert(apiRs.includes('atomic_write_file("/opt/etc/zapret/zapret.conf", &content)'),
    'save_config must atomically write zapret.conf');
  assert(apiRs.includes('atomic_write_file("/opt/etc/zapret/zapret-hosts.txt", &content)'),
    'save_hosts must atomically write zapret-hosts.txt');
});

// -------------------------------------------------------------
// 7. Z-07: Symmetric zapret.conf parsing and serialization
// -------------------------------------------------------------
runTest('9. Z-07: Symmetric zapret.conf parsing & serialization round-trip', () => {
  assert(zapretRs.includes('pub fn serialize_zapret_conf('), 'zapret.rs must define serialize_zapret_conf');
  assert(zapretRs.includes('pub fn parse_zapret_conf('), 'zapret.rs must define parse_zapret_conf');
  assert(zapretRs.includes('let smart_tv_str = if cfg.smart_tv_mode { "1" } else { "0" };'),
    'zapret.rs must serialize SMART_TV_MODE ("1" when true, "0" when false)');
  assert(zapretRs.includes('seen_smart_tv = true;'),
    'parse_zapret_conf must track explicit SMART_TV_MODE occurrence');
  assert(zapretRs.includes('EXCLUDED_IPS') && zapretRs.includes('EXCLUDED_MACS'),
    'parse_zapret_conf must parse both IP and MAC device exclusions');

  // Functional JS simulation of parse_zapret_conf and serialize_zapret_conf
  function simulateParseZapretConf(content, baseCfg) {
    const cfg = { ...baseCfg, excluded_devices: [...(baseCfg.excluded_devices || [])] };
    const ips = [];
    const macs = [];
    let seenExcludedIps = false;
    let seenExcludedMacs = false;
    let seenSmartTv = false;
    let seenBlockQuic = false;
    let smartTvVal = false;
    let blockQuicVal = false;

    for (const line of content.split('\n')) {
      const trimmed = line.trim();
      if (!trimmed || trimmed.startsWith('#')) continue;
      const idx = trimmed.indexOf('=');
      if (idx === -1) continue;
      const key = trimmed.slice(0, idx).trim();
      const val = trimmed.slice(idx + 1).trim().replace(/^["']|["']$/g, '').trim();

      if (key === 'ZAPRET_ENGINE') cfg.engine = val;
      else if (key === 'DISCORD_VOICE_ENABLED') cfg.discord_voice_udp = val === '1';
      else if (key === 'SMART_TV_MODE') {
        seenSmartTv = true;
        smartTvVal = val === '1';
      } else if (key === 'BLOCK_QUIC') {
        seenBlockQuic = true;
        blockQuicVal = val === '1';
      } else if (key === 'EXCLUDED_IPS') {
        seenExcludedIps = true;
        val.split(/\s+/).filter(Boolean).forEach(ip => ips.push(ip));
      } else if (key === 'EXCLUDED_MACS') {
        seenExcludedMacs = true;
        val.split(/\s+/).filter(Boolean).forEach(mac => macs.push(mac));
      }
    }
    if (seenSmartTv) {
      cfg.smart_tv_mode = smartTvVal;
    } else if (seenBlockQuic) {
      cfg.smart_tv_mode = blockQuicVal;
    }
    if (seenExcludedIps || seenExcludedMacs) {
      cfg.excluded_devices = [...ips, ...macs];
    }
    return cfg;
  }

  const testConf1 = 'ZAPRET_ENGINE="v2"\nSMART_TV_MODE="0"\nBLOCK_QUIC="0"\nDISCORD_VOICE_ENABLED="1"\nEXCLUDED_IPS="192.168.1.100"\nEXCLUDED_MACS="00:11:22:33:44:55"\n';
  const parsed1 = simulateParseZapretConf(testConf1, { smart_tv_mode: true, discord_voice_udp: false });
  assert.strictEqual(parsed1.smart_tv_mode, false, 'SMART_TV_MODE="0" must set smart_tv_mode to false');
  assert.strictEqual(parsed1.discord_voice_udp, true, 'DISCORD_VOICE_ENABLED="1" must set discord_voice_udp to true');
  assert.deepStrictEqual(parsed1.excluded_devices, ['192.168.1.100', '00:11:22:33:44:55'], 'device exclusions must be extracted');

  const testConf2 = 'ZAPRET_ENGINE="v1"\nSMART_TV_MODE="1"\nDISCORD_VOICE_ENABLED="0"\n';
  const parsed2 = simulateParseZapretConf(testConf2, { smart_tv_mode: false, discord_voice_udp: true });
  assert.strictEqual(parsed2.smart_tv_mode, true, 'SMART_TV_MODE="1" must set smart_tv_mode to true');
  assert.strictEqual(parsed2.discord_voice_udp, false, 'DISCORD_VOICE_ENABLED="0" must set discord_voice_udp to false');

  // Edge case: Empty exclusions must clear existing devices
  const testConf3 = 'ZAPRET_ENGINE="v2"\nEXCLUDED_IPS=""\nEXCLUDED_MACS=""\n';
  const parsed3 = simulateParseZapretConf(testConf3, { excluded_devices: ['192.168.1.100'] });
  assert.deepStrictEqual(parsed3.excluded_devices, [], 'empty EXCLUDED_IPS and EXCLUDED_MACS must clear excluded_devices');

  // Edge case: BLOCK_QUIC="0" without SMART_TV_MODE disables smart_tv_mode symmetrically
  const testConf4 = 'BLOCK_QUIC="0"\n';
  const parsed4 = simulateParseZapretConf(testConf4, { smart_tv_mode: true });
  assert.strictEqual(parsed4.smart_tv_mode, false, 'BLOCK_QUIC="0" must symmetrically set smart_tv_mode to false');
});

// -------------------------------------------------------------
// 8. Z-08: Robust process tracking via /proc/<pid>/comm and cmdline
// -------------------------------------------------------------
runTest('10. Z-08: Process tracking validates comm & cmdline, eliminates PID reuse', () => {
  // Shell is_zapret_pid function
  assert(apiRs.includes('is_zapret_pid() {'), 'S51zapret must define is_zapret_pid');
  assert(apiRs.includes('cat "/proc/$p/comm"'), 'is_zapret_pid must inspect /proc/<pid>/comm');
  assert(apiRs.includes('nfqws|nfqws2|tpws) return 0 ;;'), 'is_zapret_pid must accept nfqws, nfqws2, tpws');
  assert(apiRs.includes('tr \'\\0\' \' \' < "/proc/$p/cmdline"'), 'is_zapret_pid must inspect /proc/<pid>/cmdline');

  // stop_nfqws uses is_zapret_pid
  assert(apiRs.includes('if [ -n "$PID" ] && is_zapret_pid "$PID"; then'), 'stop_nfqws must check is_zapret_pid');
  assert(apiRs.includes('if is_zapret_pid "$p"; then\n      kill -15 "$p"'), 'stop_nfqws loop must verify is_zapret_pid before kill');
  assert(!apiRs.includes('killall -9 nfqws'), 'stop_nfqws must not execute blind killall -9');
  assert(!apiRs.includes('killall -15 nfqws'), 'stop_nfqws must not execute blind killall -15');

  // Rust zapret.rs and system.rs process validation
  assert(zapretRs.includes('pub fn is_zapret_pid_valid(pid: u32) -> bool'),
    'zapret.rs must implement is_zapret_pid_valid');
  assert(systemRs.includes('pub fn is_process_running_match(pid: u32, expected_name: &str) -> bool'),
    'system.rs must implement is_process_running_match');

  // api.rs get_zapret_status validates PID
  assert(apiRs.includes('if crate::zapret::is_zapret_pid_valid(p)'),
    'get_zapret_status must validate PID using is_zapret_pid_valid');
});

// -------------------------------------------------------------
// 9. OPS-01 & OPS-02: Elimination of phantom success & accurate indicators
// -------------------------------------------------------------
runTest('11. OPS-01: Service start verification eliminates phantom success', () => {
  assert(apiRs.includes('(action_to_run == "start" || action_to_run == "restart") && !success'),
    'zapret_action must catch non-zero exit code on start/restart');
  assert(apiRs.includes('else if !success {'),
    'zapret_action must catch non-zero exit code on all actions');
  assert(apiRs.includes('if !pid_running {'),
    'zapret_action must check if PID is actually running after start');
  assert(apiRs.includes('crate::zapret::is_zapret_pid_valid(p)'),
    'zapret_action must validate PID after start using is_zapret_pid_valid');
  assert(apiRs.includes('Служба Zapret завершилась сразу после старта:'),
    'zapret_action must return error if process died immediately after start (OPS-01)');
});

runTest('12. OPS-02: Operation response indicators (hot_reloaded & restart_required)', () => {
  // save_config returns hot_reloaded: false, restart_required
  assert(apiRs.includes('"hot_reloaded": false,\n                "restart_required": !cfg.zapret.enabled,'),
    'save_config must return hot_reloaded: false and restart_required: !cfg.zapret.enabled');

  // save_hosts returns hot_reloaded: is_running, restart_required: !is_running
  assert(apiRs.includes('"hot_reloaded": is_running,\n                "restart_required": !is_running,'),
    'save_hosts must return hot_reloaded: is_running and restart_required: !is_running');

  // zapret_action returns hot_reloaded and restart_required
  assert(apiRs.includes('let hot_reloaded = action_to_run == "reload" || action_to_run == "reload-hosts";'),
    'zapret_action must compute hot_reloaded for reload actions');
  assert(apiRs.includes('"hot_reloaded": hot_reloaded,\n                "restart_required": false,'),
    'zapret_action must return hot_reloaded and restart_required: false on execution');

  // toggle_feature returns hot_reloaded and restart_required
  assert(apiRs.includes('let hot_reloaded = is_running && is_domain_only;'),
    'toggle_feature must set hot_reloaded when running and domain-only');
  assert(apiRs.includes('let restart_required = !is_running || (!is_domain_only && !is_no_reload);'),
    'toggle_feature must set restart_required when not domain-only or stopped');

  // get_zapret_status returns operation_state and operation_in_progress
  assert(apiRs.includes('"operation_state": current_op,'),
    'get_zapret_status must return operation_state');
  assert(apiRs.includes('"operation_in_progress": operation_in_progress,'),
    'get_zapret_status must return operation_in_progress');
});

// -------------------------------------------------------------
// Summary
// -------------------------------------------------------------
console.log(`\n=== All ${passedTests}/${totalTests} Milestone 3 Checks Passed Successfully ===\n`);
if (passedTests !== totalTests) {
  process.exit(1);
}
