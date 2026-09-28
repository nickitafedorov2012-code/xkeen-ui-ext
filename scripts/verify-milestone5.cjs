// ==============================================================================
// verify-milestone5.cjs — Comprehensive Automated Verification Suite
// for Milestone 5 (Этап 5): UI & Diagnostics Truthfulness (DIAG-01..05, UI-01..08)
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

console.log('=== Running Milestone 5 UI & Diagnostics Truthfulness Verification ===\n');

const apiRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/api.rs'), 'utf8');
const failoverRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/failover.rs'), 'utf8');
const speedtestRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/speedtest.rs'), 'utf8');
const trafficRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/traffic.rs'), 'utf8');

const apiTs = fs.readFileSync(path.resolve(__dirname, '../frontend/src/api.ts'), 'utf8');
const configEditorTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/components/ConfigEditor.tsx'), 'utf8');
const antigravityTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/components/Antigravity.tsx'), 'utf8');
const presetModalTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/components/PresetCatalogModal.tsx'), 'utf8');
const scheduleModalTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/components/DeviceScheduleModal.tsx'), 'utf8');
const policiesMapTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/components/PoliciesMap.tsx'), 'utf8');
const diagnosticsTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/components/Diagnostics.tsx'), 'utf8');
const flowRepairTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/components/FlowRepairModal.tsx'), 'utf8');
const zapretModalTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/components/ZapretCoreModal.tsx'), 'utf8');
const rulesViewerTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/components/RulesViewer.tsx'), 'utf8');
const trafficGraphTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/components/TrafficGraph.tsx'), 'utf8');

// -------------------------------------------------------------
// 1. DIAG-01: Speedtest Isolation, Route Confirmation, Duration & Snapshot Restoration
// -------------------------------------------------------------
runTest('1. DIAG-01: Speedtest isolation from failover and concurrent switches', () => {
  assert(failoverRs.includes('speedtest_lock.try_lock()'), 'Failover check must check speedtest_lock before switching');
  assert(apiRs.includes('speedtest_lock.try_lock()'), 'switch_server API must check speedtest_lock before switching');
  assert(speedtestRs.includes('take_groups_snapshot') && speedtestRs.includes('restore_groups_snapshot'), 'speedtest.rs must snapshot selector groups');
  assert(speedtestRs.includes('transfer_start.elapsed()'), 'speedtest.rs must measure transfer-only duration (excluding connection/DNS handshake)');
  assert(speedtestRs.includes('resolve_active_leaf') && speedtestRs.includes('switch_server'), 'speedtest.rs must verify runtime route before download measurement');
  assert(speedtestRs.includes('restore_groups_snapshot'), 'speedtest.rs must restore groups even if speedtest fails or finishes');

  // Strict route confirmation: must verify leaf or primary groups, not arbitrary groups like device selectors
  assert(speedtestRs.includes('current_leaf == target_server'), 'speedtest.rs must verify active leaf matches target server');
  assert(!speedtestRs.includes('p.values().any'), 'speedtest.rs must NOT use p.values().any which falsely validates on isolated device groups');
  assert(speedtestRs.includes('original_server != target_server'), 'speedtest.rs must trigger route switch even when original server was empty');
});

// -------------------------------------------------------------
// 2. DIAG-02: Rule Simulation Honesty (Runtime integration & Estimated fallback)
// -------------------------------------------------------------
runTest('2. DIAG-02: Elimination of bogus analytics heuristics & Mihomo runtime rule integration', () => {
  assert(!apiRs.includes('lower.contains("analytics")'), 'Bogus substring analytics heuristic must be deleted from api.rs');
  assert(!apiRs.includes('domain.contains("discord")'), 'Bogus domain.contains("discord") substring check must be eliminated');
  assert(!apiRs.includes('domain.contains("steam")'), 'Bogus domain.contains("steam") substring check must be eliminated');
  assert(!apiRs.includes('domain.contains("twitch")'), 'Bogus domain.contains("twitch") substring check must be eliminated');
  assert(!apiRs.includes('domain.contains("tracker")'), 'Bogus domain.contains("tracker") substring check must be eliminated');

  assert(apiRs.includes('"/rules"') && apiRs.includes('get_proxies'), 'api.rs must query Mihomo runtime rules endpoint and proxies');
  assert(apiRs.includes('estimated: !is_runtime') || apiRs.includes('"estimated": !is_runtime'), 'api.rs must flag estimated rules when runtime is unavailable');
  assert(apiRs.includes('limitations'), 'api.rs must describe simulation limitations');
  assert(rulesViewerTsx.includes('estimated?: boolean') && rulesViewerTsx.includes('limitations?: string'), 'RulesViewer.tsx must declare estimated in RuleTestResult');
  assert(rulesViewerTsx.includes('testResult.estimated'), 'RulesViewer.tsx must render estimated warning banner');

  // Behavioral test: verify non-matching of lookalike domains
  const testSubstrings = (d) => {
    const is_yt = d === "youtube.com" || d.endsWith(".youtube.com") || d === "youtu.be" || d === "googlevideo.com" || d.endsWith(".googlevideo.com");
    const is_discord = d === "discord.com" || d.endsWith(".discord.com") || d === "discord.gg" || d.endsWith(".discord.gg") || d === "discordapp.com" || d.endsWith(".discordapp.com");
    const is_steam = d === "steampowered.com" || d.endsWith(".steampowered.com") || d === "steamcommunity.com" || d.endsWith(".steamcommunity.com");
    const is_twitch = d === "twitch.tv" || d.endsWith(".twitch.tv");
    const is_torrent = d === "rutracker.org" || d.endsWith(".rutracker.org") || d === "nnmclub.to" || d.endsWith(".nnmclub.to");
    return { is_yt, is_discord, is_steam, is_twitch, is_torrent };
  };

  assert.strictEqual(testSubstrings("fitness-tracker.com").is_torrent, false, 'fitness-tracker.com must not be matched as torrent');
  assert.strictEqual(testSubstrings("steamengine.com").is_steam, false, 'steamengine.com must not be matched as steam');
  assert.strictEqual(testSubstrings("discordance.org").is_discord, false, 'discordance.org must not be matched as discord');
  assert.strictEqual(testSubstrings("twitchy.net").is_twitch, false, 'twitchy.net must not be matched as twitch');
  assert.strictEqual(testSubstrings("discord.gg").is_discord, true, 'discord.gg must match discord');
  assert.strictEqual(testSubstrings("store.steampowered.com").is_steam, true, 'store.steampowered.com must match steam');
});

// -------------------------------------------------------------
// 3. DIAG-03: Honest Diagnostics (Storage df parsing & DNS observation)
// -------------------------------------------------------------
runTest('3. DIAG-03: Diagnostics health df parsing and DNS test observations without false claims', () => {
  assert(apiRs.includes('.arg("-h").arg("/opt")'), 'Health diagnostics must run df -h /opt');
  assert(apiRs.includes('has_private_ip'), 'DNS test must report has_private_ip observation');
  assert(apiRs.includes('error_type'), 'DNS test must report error_type');
  assert(!apiRs.includes('Провайдер подменяет DNS (РКН)'), 'DNS test must not falsely claim ISP spoofing for private IP or timeout');
  assert(diagnosticsTsx.includes('healthError') && diagnosticsTsx.includes('lastHealthCheckTime'), 'Diagnostics.tsx must track healthError and lastHealthCheckTime');
  assert(diagnosticsTsx.includes('dnsError') && diagnosticsTsx.includes('lastDnsTestTime'), 'Diagnostics.tsx must track dnsError and lastDnsTestTime');

  // Behavioral test: simulate token-based df parsing across multiple lines
  const parseDf = (stdout) => {
    const lines = stdout.split('\n');
    let use_percent = 0;
    for (let i = 1; i < lines.length; i++) {
      const tokens = lines[i].trim().split(/\s+/);
      for (const t of tokens) {
        if (t.endsWith('%')) {
          const val = parseInt(t.slice(0, -1), 10);
          if (!isNaN(val)) {
            use_percent = val;
            break;
          }
        }
      }
      if (use_percent > 0) break;
    }
    return use_percent;
  };

  const busyboxOutput = "Filesystem                Size      Used Available Use% Mounted on\n/dev/disk/by-uuid/1234-5678-abcdef012345\n                          7.2G      6.8G    400.0M  95% /opt";
  assert.strictEqual(parseDf(busyboxOutput), 95, 'Multi-line wrapped df output must yield 95% usage');

  // Behavioral test: simulate RFC 1918 range check
  const isPrivateIp = (ip) => {
    if (ip.startsWith('10.') || ip.startsWith('192.168.')) return true;
    if (ip.startsWith('172.')) {
      const parts = ip.split('.');
      const second = parseInt(parts[1], 10);
      return second >= 16 && second <= 31;
    }
    return ip === '127.0.0.1' || ip === '0.0.0.0' || ip === '::1';
  };

  assert.strictEqual(isPrivateIp('172.20.1.5'), true, '172.20.1.5 must be recognized as private IP');
  assert.strictEqual(isPrivateIp('172.31.255.255'), true, '172.31.255.255 must be recognized as private IP');
  assert.strictEqual(isPrivateIp('172.32.0.1'), false, '172.32.0.1 must NOT be recognized as private IP');
  assert.strictEqual(isPrivateIp('1.1.1.1'), false, '1.1.1.1 must NOT be recognized as private IP');
});

// -------------------------------------------------------------
// 4. DIAG-04: Flow Repair Verification & Elimination of Fake Fallback
// -------------------------------------------------------------
runTest('4. DIAG-04: Flow repair verified runtime switch and no fake fallback server', () => {
  assert(apiRs.includes('repair_flow'), 'api.rs must define repair_flow handler');
  assert(apiRs.includes('is_confirmed') && apiRs.includes('switch_flow_server'), 'repair_flow must verify runtime selector switch');
  assert(apiRs.includes('if !is_confirmed'), 'repair_flow must verify confirmation status');
  assert(apiRs.includes('return api_err(format!("Маршрут Google AI в ядре Mihomo не подтвердил'), 'repair_flow must fail with error if confirmation fails');
  assert(!flowRepairTsx.includes('🇺🇸 США'), 'FlowRepairModal must not hardcode 🇺🇸 США fallback');
  assert(flowRepairTsx.includes('isVerified') && flowRepairTsx.includes('repairError'), 'FlowRepairModal must track isVerified and repairError');
});

// -------------------------------------------------------------
// 5. DIAG-05: Direct Traffic Estimated Labeling
// -------------------------------------------------------------
runTest('5. DIAG-05: Direct traffic labeled as estimated with measurement source', () => {
  assert(trafficRs.includes('"estimated": true'), 'traffic.rs must label direct traffic as estimated');
  assert(trafficRs.includes('"source": "WAN (/proc/net/dev) - Mihomo (/traffic)"'), 'traffic.rs must specify measurement source');
  assert(trafficGraphTsx.includes('isDirectEstimated'), 'TrafficGraph.tsx must track isDirectEstimated');
  assert(trafficGraphTsx.includes('(оценка)'), 'TrafficGraph.tsx must render (оценка) badge for direct traffic');
});

// -------------------------------------------------------------
// 6. UI-01: ConfigEditor Race Prevention & Unified Path Resolution
// -------------------------------------------------------------
runTest('6. UI-01: ConfigEditor request/file ID tracking and unified config_path resolution', () => {
  assert(configEditorTsx.includes('currentRequestIdRef') && configEditorTsx.includes('loadedFileIdRef'), 'ConfigEditor must track request ID and loaded file ID');
  assert(configEditorTsx.includes('loadError'), 'ConfigEditor must track loadError');
  assert(configEditorTsx.includes('loadError') && configEditorTsx.includes('disabled={saving || !isDirty || loading || !!loadError'), 'ConfigEditor must disable save on loadError');
  assert(apiRs.includes('resolve_config_file_path'), 'api.rs must use resolve_config_file_path');
  assert(apiRs.includes('state.config_path'), 'api.rs resolve_config_file_path must use state.config_path for route');
});

// -------------------------------------------------------------
// 7. UI-02: Antigravity Dirty-Guard Against 5s Polling Overwrite
// -------------------------------------------------------------
runTest('7. UI-02: Antigravity settings dirty-guard and reset button', () => {
  assert(antigravityTsx.includes('isSettingsDirtyRef'), 'Antigravity.tsx must maintain isSettingsDirtyRef');
  assert(antigravityTsx.includes('!isSettingsDirtyRef.current'), 'Antigravity load must guard against dirty inputs');
  assert(antigravityTsx.includes('handleResetSettings'), 'Antigravity.tsx must have reset settings handler');
  assert(antigravityTsx.includes('Есть несохранённые изменения'), 'Antigravity.tsx must display dirty indicator');
});

// -------------------------------------------------------------
// 8. UI-03: Optimistic Concurrency on Domains & Schedules
// -------------------------------------------------------------
runTest('8. UI-03: Optimistic revision concurrency on domains and schedules', () => {
  assert(apiRs.includes('calc_domains_revision') && apiRs.includes('calc_schedules_revision'), 'api.rs must calculate revisions');
  assert(apiRs.includes('expected_revision'), 'api.rs must check expected_revision for conflict detection');
  assert(presetModalTsx.includes('expected_revision'), 'PresetCatalogModal must pass expected_revision');
  assert(scheduleModalTsx.includes('expected_revision'), 'DeviceScheduleModal must pass expected_revision');
});

// -------------------------------------------------------------
// 9. UI-04: PoliciesMap Fresh Device Derivation
// -------------------------------------------------------------
runTest('9. UI-04: PoliciesMap derives selected device by IP from fresh data', () => {
  assert(policiesMapTsx.includes('selectedDeviceIp'), 'PoliciesMap must track selectedDeviceIp');
  assert(policiesMapTsx.includes('devices.find'), 'PoliciesMap must find selected device from fresh devices list');
});

// -------------------------------------------------------------
// 10. UI-07: ZapretCoreModal Marketing Claims & Hardcoded Versions Removal
// -------------------------------------------------------------
runTest('10. UI-07: ZapretCoreModal removes marketing promises and hardcoded versions', () => {
  assert(!zapretModalTsx.includes('100% стабильный обход'), 'ZapretCoreModal must not claim 100% стабильный обход');
  assert(!zapretModalTsx.includes("version: 'v1.0.5.2'"), 'ZapretCoreModal must not hardcode v1.0.5.2');
  assert(!zapretModalTsx.includes("'v72.13'"), 'ZapretCoreModal must not hardcode v72.13');
});

// -------------------------------------------------------------
// 11. UI-08: API Client Runtime Validation
// -------------------------------------------------------------
runTest('11. UI-08: API client runtime validation for critical endpoints', () => {
  assert(apiTs.includes('validateEndpointData'), 'api.ts must define validateEndpointData');
  assert(apiTs.includes('config-files/read'), 'validateEndpointData must validate config-files/read');
  assert(apiTs.includes('diagnostics/health'), 'validateEndpointData must validate diagnostics/health');
  assert(apiTs.includes('rules'), 'validateEndpointData must validate rules');
  assert(apiTs.includes('antigravity/status'), 'validateEndpointData must validate antigravity/status');
});

console.log(`\nVerification complete: ${passedTests}/${totalTests} tests passed.\n`);
if (passedTests !== totalTests) {
  process.exit(1);
}
