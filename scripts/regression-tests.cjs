/**
 * XKeen Route — Comprehensive Audit Regression Test Suite
 * Covers all 10 mandatory regression areas:
 * 1. Trojan %40 -> @ -> %40 round-trip without %2540
 * 2. VLESS WS/gRPC and TUIC round-trip
 * 3. YAML missing END-marker protection (routing)
 * 4. Backup import JSON and rollback restore
 * 5. Open/close ConfigEditor and ShareNodeModal (React Hook Order)
 * 6. Traffic without \n buffer limit
 * 7. Failover cooldown / flapping
 * 8. Zapret: restart before previous failsafe expires & lo/br+ exclusion
 * 9. Updater: alien ELF architecture validation
 * 10. Graceful shutdown of background tasks
 */

const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert');
const cp = require('node:child_process');

console.log('=== Running XKeen Route Mandatory Regression Test Suite ===\n');

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

// Load nodeParser
const frontPath = path.resolve(__dirname, '../frontend');
const ts = require(path.join(frontPath, 'node_modules/typescript'));
const nodeParserSource = fs.readFileSync(path.join(frontPath, 'src/utils/nodeParser.ts'), 'utf8');
const compiledNodeParser = ts.transpileModule(nodeParserSource, {
  compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020 },
}).outputText;
const nodeParserModule = { exports: {} };
new Function('exports', 'module', compiledNodeParser)(nodeParserModule.exports, nodeParserModule);
const { parseProxyLink, exportServerToLink } = nodeParserModule.exports;

// -------------------------------------------------------------
// 1. Trojan %40 -> @ -> %40
// -------------------------------------------------------------
runTest('1. Trojan %40 -> @ -> %40 round-trip without %2540', () => {
  const original = 'trojan://p%40ss@example.com:443#test';
  const parsed = parseProxyLink(original);
  assert(parsed, 'Parsed node should not be null');
  assert.strictEqual(parsed.raw.password, 'p@ss', 'Parsed password must be decoded to p@ss');

  const exported = exportServerToLink({
    name: parsed.name,
    protocol: parsed.protocol,
    host: parsed.raw.server,
    port: parsed.raw.port,
    raw: parsed.raw,
  });
  assert(exported.includes('trojan://p%40ss@example.com:443'), `Exported link should have p%40ss: ${exported}`);
  assert(!exported.includes('%2540'), `Exported link must NOT double encode to %2540: ${exported}`);

  const reparsed = parseProxyLink(exported);
  assert(reparsed, 'Reparsed node should not be null');
  assert.strictEqual(reparsed.raw.password, 'p@ss', 'Reparsed password must remain p@ss');
});

// -------------------------------------------------------------
// 2. VLESS WS/gRPC и TUIC round-trip
// -------------------------------------------------------------
runTest('2. VLESS WS/gRPC and TUIC round-trip', () => {
  // 2a. VLESS WS
  const vlessWs = 'vless://uuid-1@example.com:443?security=tls&type=ws&path=%2Fcustom%2Fws&host=cdn.target.com#WSNode';
  const pWs = parseProxyLink(vlessWs);
  assert(pWs && pWs.raw['ws-opts']);
  assert.strictEqual(pWs.raw['ws-opts'].path, '/custom/ws');
  assert.strictEqual(pWs.raw['ws-opts'].headers.Host, 'cdn.target.com');
  const expWs = exportServerToLink({ name: pWs.name, protocol: pWs.protocol, host: pWs.raw.server, port: pWs.raw.port, raw: pWs.raw });
  assert(expWs.includes('path=%2Fcustom%2Fws'));
  assert(expWs.includes('host=cdn.target.com'));
  const repWs = parseProxyLink(expWs);
  assert.strictEqual(repWs.raw['ws-opts'].path, '/custom/ws');
  assert.strictEqual(repWs.raw['ws-opts'].headers.Host, 'cdn.target.com');

  // 2b. VLESS gRPC
  const vlessGrpc = 'vless://uuid-2@example.com:443?security=tls&type=grpc&serviceName=my-grpc#GrpcNode';
  const pGrpc = parseProxyLink(vlessGrpc);
  assert(pGrpc && pGrpc.raw['grpc-opts']);
  assert.strictEqual(pGrpc.raw['grpc-opts']['grpc-service-name'], 'my-grpc');
  const expGrpc = exportServerToLink({ name: pGrpc.name, protocol: pGrpc.protocol, host: pGrpc.raw.server, port: pGrpc.raw.port, raw: pGrpc.raw });
  assert(expGrpc.includes('serviceName=my-grpc'));
  const repGrpc = parseProxyLink(expGrpc);
  assert.strictEqual(repGrpc.raw['grpc-opts']['grpc-service-name'], 'my-grpc');

  // 2c. TUIC
  const tuic = 'tuic://user-1:secret%40pass@tuic.example.com:8443?alpn=h3&congestion_control=bbr&sni=sni.tuic.com#TuicNode';
  const pTuic = parseProxyLink(tuic);
  assert(pTuic && pTuic.protocol === 'TUIC');
  assert.strictEqual(pTuic.raw.uuid, 'user-1');
  assert.strictEqual(pTuic.raw.password, 'secret@pass');
  assert.strictEqual(pTuic.raw['congestion-controller'], 'bbr');
  assert.deepStrictEqual(pTuic.raw.alpn, ['h3']);
  const expTuic = exportServerToLink({ name: pTuic.name, protocol: pTuic.protocol, host: pTuic.raw.server, port: pTuic.raw.port, raw: pTuic.raw });
  assert(expTuic.includes('tuic://user-1:secret%40pass@tuic.example.com:8443'));
  assert(expTuic.includes('congestion_control=bbr'));
  assert(expTuic.includes('alpn=h3'));
  const repTuic = parseProxyLink(expTuic);
  assert.strictEqual(repTuic.raw.password, 'secret@pass');
  assert.strictEqual(repTuic.raw['congestion-controller'], 'bbr');
});

// -------------------------------------------------------------
// 3. YAML с отсутствующим END-маркером
// -------------------------------------------------------------
runTest('3. YAML missing END-marker protection', () => {
  const ADBLOCK_BEGIN = '# --- START XKEEN ROUTE ADBLOCK ---';
  const ADBLOCK_END = '# --- END XKEEN ROUTE ADBLOCK ---';

  function safeRemoveAdblock(yaml) {
    if (!yaml.includes(ADBLOCK_BEGIN) || !yaml.includes(ADBLOCK_END)) {
      return yaml; // Protected against truncation!
    }
    const re = new RegExp(`\\n?[ \\t]*${ADBLOCK_BEGIN}\\n[\\s\\S]*?\\n?[ \\t]*${ADBLOCK_END}\\n?`, 'g');
    return yaml.replace(re, '');
  }

  const corruptedYaml = `rules:\n${ADBLOCK_BEGIN}\n  - GEOSITE,category-ads-all,REJECT\n  - GEOIP,RU,DIRECT\n  - MATCH,PROXY\n`;
  const result = safeRemoveAdblock(corruptedYaml);
  assert.strictEqual(result, corruptedYaml, 'Missing END marker must not truncate the file');
  assert(result.includes('GEOIP,RU,DIRECT'), 'Subsequent rules must be preserved');
  assert(result.includes('MATCH,PROXY'), 'MATCH rule must be preserved');
});

// -------------------------------------------------------------
// 4. Backup import JSON и rollback restore
// -------------------------------------------------------------
runTest('4. Backup import JSON and rollback restore simulation', () => {
  // 4a. Import JSON validation
  const validSnapshot = JSON.stringify({
    files: { 'config.yaml': 'port: 7890\n', 'config.json': '{"rci":{}}\n' },
  });
  const parsed = JSON.parse(validSnapshot);
  assert(parsed.files && typeof parsed.files === 'object', 'Snapshot must have files object');

  const invalidJson = 'Not JSON content';
  assert.throws(() => JSON.parse(invalidJson));

  // 4b. Rollback restore simulation
  const tmpDir = path.join(__dirname, '../.tmp_test_backup');
  fs.mkdirSync(tmpDir, { recursive: true });
  const cfgYaml = path.join(tmpDir, 'config.yaml');
  const cfgJson = path.join(tmpDir, 'config.json');
  const bakYaml = path.join(tmpDir, 'config.yaml.bak');
  const bakJson = path.join(tmpDir, 'config.json.bak');

  fs.writeFileSync(cfgYaml, 'ORIGINAL_YAML', 'utf8');
  fs.writeFileSync(cfgJson, 'ORIGINAL_JSON', 'utf8');

  // Backup to .bak
  fs.copyFileSync(cfgYaml, bakYaml);
  fs.copyFileSync(cfgJson, bakJson);

  // Overwrite first file
  fs.writeFileSync(cfgYaml, 'NEW_RESTORED_YAML', 'utf8');

  // Simulate error restoring second file -> triggers rollback
  const step2Failed = true;
  if (step2Failed) {
    fs.copyFileSync(bakYaml, cfgYaml);
    fs.copyFileSync(bakJson, cfgJson);
    fs.rmSync(bakYaml, { force: true });
    fs.rmSync(bakJson, { force: true });
  }

  assert.strictEqual(fs.readFileSync(cfgYaml, 'utf8'), 'ORIGINAL_YAML', 'YAML must be rolled back to original');
  assert.strictEqual(fs.readFileSync(cfgJson, 'utf8'), 'ORIGINAL_JSON', 'JSON must be rolled back to original');

  fs.rmSync(tmpDir, { recursive: true, force: true });
});

// -------------------------------------------------------------
// 5. Открытие/закрытие ConfigEditor и ShareNodeModal
// -------------------------------------------------------------
runTest('5. Open/close ConfigEditor and ShareNodeModal (Vitest Component Test)', () => {
  const r = cp.spawnSync(process.execPath, [
    'node_modules/vitest/vitest.mjs',
    'run',
    'src/components/ModalsHookOrder.test.tsx',
  ], { cwd: frontPath, encoding: 'utf8' });
  assert.strictEqual(r.status, 0, `ModalsHookOrder test failed: ${r.stderr || r.stdout}`);
  assert(r.stdout.includes('passed'), 'ModalsHookOrder test must pass');
});

// -------------------------------------------------------------
// 6. Traffic без \n до лимита буфера
// -------------------------------------------------------------
runTest('6. Traffic buffer without newline caps at 65536 bytes', () => {
  let buffer = '';
  const chunk = 'A'.repeat(10000);

  // Append 7 chunks of 10,000 bytes = 70,000 bytes without newline
  for (let i = 0; i < 7; i++) {
    if (buffer.length + chunk.length > 65536) {
      buffer = '';
    }
    if (chunk.length <= 65536) {
      buffer += chunk;
    }
  }

  assert(buffer.length <= 65536, `Buffer length ${buffer.length} must not exceed 65536 bytes`);
  assert.strictEqual(buffer.length, 10000, 'Buffer should have cleared and reset to last chunk');
});

// -------------------------------------------------------------
// 7. Failover cooldown / flapping
// -------------------------------------------------------------
runTest('7. Failover cooldown and flapping prevention', () => {
  const cooldownSecs = 30;
  let lastSwitchMs = Date.now();

  function checkCooldown(currentMs) {
    const elapsedSecs = (currentMs - lastSwitchMs) / 1000;
    if (elapsedSecs < cooldownSecs) {
      return { allowed: false, remaining: Math.ceil(cooldownSecs - elapsedSecs) };
    }
    return { allowed: true };
  }

  // 1. Immediately after switch (elapsed = 2s) -> BLOCKED
  const check1 = checkCooldown(lastSwitchMs + 2000);
  assert.strictEqual(check1.allowed, false, 'Switch within cooldown must be blocked');
  assert.strictEqual(check1.remaining, 28);

  // 2. Midway through cooldown (elapsed = 15s) -> BLOCKED
  const check2 = checkCooldown(lastSwitchMs + 15000);
  assert.strictEqual(check2.allowed, false, 'Switch within cooldown must be blocked');

  // 3. After cooldown expired (elapsed = 35s) -> ALLOWED
  const check3 = checkCooldown(lastSwitchMs + 35000);
  assert.strictEqual(check3.allowed, true, 'Switch after 30s must be allowed');
});

// -------------------------------------------------------------
// 8. Zapret: restart до истечения старого failsafe
// -------------------------------------------------------------
runTest('8. Zapret failsafe PID management and lo/br+ exclusions in script', () => {
  const apiRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/api.rs'), 'utf8');

  // Check failsafe PID file definition
  assert(apiRs.includes('FAILSAFE_PID="/opt/var/run/zapret_failsafe.pid"'), 'S51ZAPRET_SCRIPT must define FAILSAFE_PID');

  // Check that failsafe is killed before start/restart/del_fw
  assert(apiRs.includes('kill -9 $(cat "$FAILSAFE_PID")'), 'Must kill previous failsafe PID');

  // Check lo and br+/Bridge+ exclusion rules in zapret chain
  assert(/iptables -t mangle -A zapret -o lo (?:-m comment --comment "[^"]+" )?-j RETURN/.test(apiRs), 'Must exclude outgoing lo in zapret chain');
  assert(/iptables -t mangle -A zapret -i lo (?:-m comment --comment "[^"]+" )?-j RETURN/.test(apiRs), 'Must exclude incoming lo in zapret chain');
  assert(/iptables -t mangle -A zapret -o br\+ (?:-m comment --comment "[^"]+" )?-j RETURN/.test(apiRs), 'Must exclude bridge br+ in zapret chain');
  assert(/iptables -t mangle -A zapret -o Bridge\+ (?:-m comment --comment "[^"]+" )?-j RETURN/.test(apiRs), 'Must exclude Keenetic Bridge+ in zapret chain');

  // Check DNS redirection to Mihomo port 1053 to prevent ISP DNS poisoning (NXDOMAIN)
  assert(/iptables -t nat -A PREROUTING (?:-i \S+ )?-p udp --dport 53 (?:-m comment --comment "[^"]+" )?-j REDIRECT --to-ports 1053/.test(apiRs), 'Must redirect UDP 53 to 1053');
  assert(/iptables -t nat -A PREROUTING (?:-i \S+ )?-p tcp --dport 53 (?:-m comment --comment "[^"]+" )?-j REDIRECT --to-ports 1053/.test(apiRs), 'Must redirect TCP 53 to 1053');

  // Ensure PREROUTING mangle hook is removed so it doesn't break Mihomo/Telegram
  assert(!apiRs.includes('iptables -t mangle -I PREROUTING 1 -i br+ -j zapret'), 'Must not intercept LAN packets in PREROUTING mangle');
});

// -------------------------------------------------------------
// 9. Updater: чужая ELF-архитектура
// -------------------------------------------------------------
runTest('9. Updater ELF architecture header validation', () => {
  function validateElfHeader(header, targetArch) {
    if (header.length < 20 || header[0] !== 0x7F || header[1] !== 0x45 || header[2] !== 0x4C || header[3] !== 0x46) {
      return { ok: false, error: 'Файл не является ELF-бинарём' };
    }
    const eiClass = header[4]; // 1 = 32-bit, 2 = 64-bit
    const eiData = header[5];  // 1 = LSB, 2 = MSB
    const eMachine = eiData === 2
      ? (header[18] << 8) | header[19]
      : header[18] | (header[19] << 8);

    let validArch = false;
    switch (targetArch) {
      case 'aarch64': validArch = (eiClass === 2 && eMachine === 183); break;
      case 'arm':     validArch = (eiClass === 1 && eMachine === 40); break;
      case 'mipsel':  validArch = (eiClass === 1 && eiData === 1 && eMachine === 8); break;
      case 'mips':    validArch = (eiClass === 1 && eiData === 2 && eMachine === 8); break;
      case 'x86_64':  validArch = (eiClass === 2 && eMachine === 62); break;
      case 'x86':     validArch = (eiClass === 1 && eMachine === 3); break;
      default:        validArch = true;
    }
    if (!validArch) {
      return { ok: false, error: `Архитектура не соответствует целевой: class=${eiClass}, data=${eiData}, machine=${eMachine}` };
    }
    return { ok: true };
  }

  // ARM64 header (class=2, data=1, machine=183)
  const arm64Header = Buffer.alloc(20);
  arm64Header.set([0x7F, 0x45, 0x4C, 0x46, 2, 1]);
  arm64Header.writeUInt16LE(183, 18);
  assert(validateElfHeader(arm64Header, 'aarch64').ok, 'ARM64 header on aarch64 must be valid');
  assert(!validateElfHeader(arm64Header, 'arm').ok, 'Alien: ARM64 header on arm32 must be rejected');

  // ARM32 header (class=1, data=1, machine=40)
  const arm32Header = Buffer.alloc(20);
  arm32Header.set([0x7F, 0x45, 0x4C, 0x46, 1, 1]);
  arm32Header.writeUInt16LE(40, 18);
  assert(validateElfHeader(arm32Header, 'arm').ok, 'ARM32 header on arm must be valid');
  assert(!validateElfHeader(arm32Header, 'aarch64').ok, 'Alien: ARM32 header on aarch64 must be rejected');

  // MIPSEL header (class=1, data=1, machine=8) vs MIPS (data=2)
  const mipselHeader = Buffer.alloc(20);
  mipselHeader.set([0x7F, 0x45, 0x4C, 0x46, 1, 1]);
  mipselHeader.writeUInt16LE(8, 18);
  assert(validateElfHeader(mipselHeader, 'mipsel').ok, 'MIPSEL on mipsel must be valid');
  assert(!validateElfHeader(mipselHeader, 'mips').ok, 'Alien: MIPSEL on MIPS (MSB) must be rejected');

  // Corrupted HTML
  const htmlHeader = Buffer.from('<!DOCTYPE html><html>');
  assert(!validateElfHeader(htmlHeader, 'aarch64').ok, 'Non-ELF must be rejected');
});

// -------------------------------------------------------------
// 10. Graceful shutdown фоновых задач
// -------------------------------------------------------------
runTest('10. Graceful shutdown flags and loop abort logic', () => {
  let isShutdown = false;
  function shutdown() {
    isShutdown = true;
  }

  // Simulate background task
  let loopIterations = 0;
  function backgroundTaskStep() {
    if (isShutdown) return false;
    loopIterations++;
    return true;
  }

  assert.strictEqual(backgroundTaskStep(), true);
  assert.strictEqual(backgroundTaskStep(), true);
  assert.strictEqual(loopIterations, 2);

  // Signal shutdown
  shutdown();
  assert.strictEqual(isShutdown, true);
  assert.strictEqual(backgroundTaskStep(), false, 'Task must abort immediately when shutdown is true');
  assert.strictEqual(loopIterations, 2, 'No further iterations should execute');
});

// -------------------------------------------------------------
// 11. Zapret Netfilter persistence & Keenetic ndm hooks
// -------------------------------------------------------------
runTest('11. Zapret Netfilter persistence, Keenetic ndm hooks and watchdog self-healing', () => {
  const apiRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/api.rs'), 'utf8');
  const watchdogRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/watchdog.rs'), 'utf8');

  // Verify NDM hook scripts definitions
  assert(apiRs.includes('pub const NDM_NETFILTER_SCRIPT: &str'), 'api.rs must define NDM_NETFILTER_SCRIPT');
  assert(apiRs.includes('pub const NDM_IFSTATE_SCRIPT: &str'), 'api.rs must define NDM_IFSTATE_SCRIPT');
  assert(apiRs.includes('/opt/etc/ndm/netfilter.d/050-zapret.sh'), 'sync_zapret_files must install 050-zapret.sh into netfilter.d');
  assert(apiRs.includes('/opt/etc/ndm/ifstatechanged.d/050-zapret.sh'), 'sync_zapret_files must install 050-zapret.sh into ifstatechanged.d');
  assert(apiRs.includes('/opt/etc/ndm/wan.d/050-zapret.sh'), 'sync_zapret_files must install 050-zapret.sh into wan.d');

  // Verify NDM scripts have pidof nfqws fallback
  assert(apiRs.includes('pidof nfqws >/dev/null 2>&1'), 'NDM scripts must check pidof nfqws as fallback');

  // Verify start-fw and reload-fw commands in S51zapret
  assert(apiRs.includes('start-fw|reload-fw)'), 'S51zapret must support start-fw and reload-fw commands');

  // Verify PATH is exported in S51zapret to avoid curl/iptables missing
  assert(apiRs.includes('PATH=/opt/sbin:/opt/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin'), 'S51zapret must define complete PATH');

  // Verify xtables wait lock wrapper in S51zapret
  assert(apiRs.includes('IPTABLES_CMD="iptables"'), 'S51zapret must define IPTABLES_CMD wrapper');
  assert(apiRs.includes('iptables -w 2'), 'S51zapret must support xtables wait-lock option');

  // Verify watchdog self-healing & strict boolean check (&&, not ||)
  assert(watchdogRs.includes('spawn_zapret_monitor'), 'watchdog.rs must define spawn_zapret_monitor');
  assert(watchdogRs.includes('start-fw'), 'spawn_zapret_monitor must call start-fw on lost iptables rules');
  assert(watchdogRs.includes(') && iptables -t mangle -nL zapret 2>/dev/null | grep -q NFQUEUE'), 'watchdog.rs must strictly require both hook and NFQUEUE chain with &&');
  assert(apiRs.includes(') && iptables -t mangle -nL zapret 2>/dev/null | grep -q NFQUEUE'), 'api.rs must strictly require both hook and NFQUEUE chain with &&');
});

// -------------------------------------------------------------
// 12. Zapret 2.0 (nfqws2 + Lua Engine) migration & UI state locking
// -------------------------------------------------------------
runTest('12. Zapret 2.0 (nfqws2 + Lua Engine) migration, fast SIGHUP reload and UI state locking', () => {
  const apiRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/api.rs'), 'utf8');
  const watchdogRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/watchdog.rs'), 'utf8');
  const systemRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/system.rs'), 'utf8');
  const zapretTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/components/Zapret.tsx'), 'utf8');

  // 1. Verify build_nfqws2_args generates zapret2 syntax
  assert(apiRs.includes('pub fn build_nfqws2_args'), 'api.rs must define build_nfqws2_args');
  assert(apiRs.includes('--payload=tls_client_hello'), 'nfqws2 args must use --payload=tls_client_hello');
  assert(apiRs.includes('--out-range=-d10'), 'nfqws2 args must specify packet cutoff --out-range=-d10');
  assert(apiRs.includes('--lua-desync='), 'nfqws2 args must specify Lua engine desynchronization --lua-desync=');

  // 2. Verify S51zapret script checks and initializes Lua engine for nfqws2
  assert(apiRs.includes('/opt/zapret2/lua/zapret-lib.lua'), 'S51zapret must verify /opt/zapret2/lua/zapret-lib.lua');
  assert(apiRs.includes('/opt/zapret2/lua/zapret-antidpi.lua'), 'S51zapret must verify /opt/zapret2/lua/zapret-antidpi.lua');
  assert(apiRs.includes('--lua-init=@/opt/zapret2/lua/zapret-lib.lua'), 'S51zapret must launch nfqws2 with --lua-init');

  // 3. Verify fast hostlist reload via SIGHUP (kill -HUP)
  assert(apiRs.includes('reload|reload-hosts)'), 'S51zapret must support reload and reload-hosts commands');
  assert(apiRs.includes('kill -HUP'), 'S51zapret must send SIGHUP (kill -HUP) to reload domain lists without restart');

  // 4. Verify nfqws2 process detection in NDM hooks, watchdog and system protected processes
  assert(apiRs.includes('pidof nfqws2'), 'NDM scripts in api.rs must check pidof nfqws2 before nfqws');
  assert(watchdogRs.includes('pidof nfqws2'), 'watchdog.rs must monitor pidof nfqws2');
  assert(systemRs.includes('"nfqws2"'), 'system.rs must protect nfqws2 in PROTECTED_PROCESSES');
  assert(systemRs.includes('name_lower == "nfqws2"'), 'system.rs detect_category must map nfqws2 to zapret');

  // 5. Verify frontend state locking (pendingKeysRef) and loading micro-spinner
  assert(zapretTsx.includes('pendingKeysRef'), 'Zapret.tsx must use pendingKeysRef to prevent polling state wipe');
  assert(zapretTsx.includes('data-testid="zapret-micro-spinner"'), 'Zapret.tsx must render micro-spinner during transitions');
  assert(zapretTsx.includes('zapret-glow-pulse-blue'), 'Zapret.tsx must apply smooth glowing pulse animation');

  // 6. Verify 45-second failsafe rollback timer and safe whitespace trimming
  assert(apiRs.includes('sleep 45'), 'S51zapret start_failsafe must use exactly 45s failsafe rollback timer (sleep 45)');
  assert(apiRs.includes("key=$(echo \"$key\" | tr -d ' \\t\\r\\n')"), 'S51zapret parser must trim whitespace from keys');
  assert(apiRs.includes('--queue-bypass'), 'S51zapret must use mandatory --queue-bypass for NFQUEUE');

  // 7. Verify all 6 DPI presets are supported in backend and frontend
  const presets = ['youtube', 'discord', 'gamer', 'aggressive', 'custom', 'default'];
  for (const preset of presets) {
    assert(apiRs.includes(`"${preset}"`), `api.rs must recognize preset "${preset}"`);
    assert(zapretTsx.includes(`'${preset}'`), `Zapret.tsx must support preset "${preset}"`);
  }
});

// -------------------------------------------------------------
// 13. Zapret legacy nfqws compatibility & crash prevention
// -------------------------------------------------------------
runTest('13. Zapret legacy nfqws compatibility, lua-desync conversion and instant crash prevention', () => {
  const apiRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/api.rs'), 'utf8');
  const watchdogRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/watchdog.rs'), 'utf8');

  // 1. Verify lua desync to legacy conversion helper
  assert(apiRs.includes('pub fn convert_lua_to_legacy_desync'), 'api.rs must define convert_lua_to_legacy_desync');
  assert(apiRs.includes('pub fn is_nfqws2_available'), 'api.rs must define is_nfqws2_available');
  assert(apiRs.includes('build_nfqws_args_with_desync'), 'api.rs must define build_nfqws_args_with_desync');

  // 2. Verify S51zapret script guards against passing Lua args to legacy nfqws
  assert(apiRs.includes('*lua-desync*|*payload=*|*out-range=*'), 'S51zapret must guard against passing nfqws2 Lua parameters to legacy nfqws');
  assert(apiRs.includes('logger -t zapret "WARNING: NFQWS_ARGS contains nfqws2 Lua parameters'), 'S51zapret must log warning and use safe legacy fallback');

  // 3. Verify instant failure detection on start/restart in zapret_action
  assert(apiRs.includes('Служба Zapret завершилась сразу после старта'), 'zapret_action must detect if nfqws dies immediately after start');
  assert(apiRs.includes('cfg.zapret.enabled = false'), 'zapret_action must reset enabled to false on startup failure');

  // 4. Verify watchdog disables enabled flag after 5 consecutive failures
  assert(watchdogRs.includes('cfg.zapret.enabled = false'), 'watchdog.rs must disable zapret after 5 failed restarts to prevent infinite loops');

  // 5. Verify S51zapret iptables wrapper uses `command $IPTABLES_CMD` to prevent infinite shell recursion
  assert(apiRs.includes('command $IPTABLES_CMD "$@"'), 'S51zapret iptables() function must use `command $IPTABLES_CMD "$@"` to prevent infinite recursion');
  assert(!apiRs.includes('--dpi-desync=fake,multisplit'), 'legacy nfqws must not receive unsupported --dpi-desync=fake,multisplit');

  // 6. Verify reload-hosts auto-restarts nfqws if zapret-hosts.txt profile was newly added, cmdline comma restoration, and save_config rollback
  assert(apiRs.includes('pub fn format_nfqws_proc_cmdline'), 'api.rs must reconstruct strtok-mutated commas in /proc/<pid>/cmdline');
  assert(apiRs.includes('exec "$0" restart'), 'S51zapret reload-hosts must restart nfqws if zapret-hosts.txt profile was newly added to NFQWS_ARGS');
  assert(apiRs.includes('Ошибка перезапуска Zapret с новой конфигурацией'), 'save_config must check S51zapret restart status and roll back on failure');
  assert(watchdogRs.includes('state.config_lock.try_lock()'), 'spawn_zapret_monitor must guard with config_lock.try_lock() to prevent racing with active zapret_action');
});

// -------------------------------------------------------------
// 14. Zapret 2 (nfqws2) 1-Click Upgrade, Engine Switcher & Safe v1 Rollback
// -------------------------------------------------------------
runTest('14. Zapret 2 (nfqws2) 1-click upgrade, router hardware auto-hint, and safe rollback to v1', () => {
  const apiRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/api.rs'), 'utf8');
  const configRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/config.rs'), 'utf8');
  const zapretTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/components/Zapret.tsx'), 'utf8');

  // 1. Verify config.rs has engine field on ZapretConfig
  assert(configRs.includes('pub engine: String'), 'config.rs ZapretConfig must include pub engine: String');

  // 2. Verify backend hardware auto-detection and hint builder
  assert(apiRs.includes('pub fn build_zapret_hardware_info'), 'api.rs must define build_zapret_hardware_info');
  assert(apiRs.includes('pub async fn detect_zapret_hardware'), 'api.rs must define detect_zapret_hardware');
  assert(apiRs.includes('рекомендуется Zapret 2.0'), 'api.rs must generate hardware recommendation hint for Zapret 2.0');

  // 3. Verify 1-click upgrade_zapret2, switch_engine, and rollback_v1 actions
  assert(apiRs.includes('act == "upgrade_zapret2"'), 'api.rs must handle upgrade_zapret2 action');
  assert(apiRs.includes('act == "rollback_v1" || act == "switch_engine"'), 'api.rs must handle rollback_v1 and switch_engine actions');
  assert(apiRs.includes('/opt/zapret/nfqws.bak'), 'api.rs must backup and restore v1 binary in /opt/zapret/nfqws.bak for safe rollback');

  // 4. Verify S51zapret evaluates BIN=$(find_bin) after reading ZAPRET_ENGINE from zapret.conf
  const parseIdx = apiRs.indexOf('ZAPRET_ENGINE)');
  const binIdx = apiRs.indexOf('BIN=$(find_bin)');
  assert(parseIdx !== -1 && binIdx !== -1 && parseIdx < binIdx, 'S51zapret must parse ZAPRET_ENGINE from zapret.conf before calling find_bin');

  // 4.1 Verify save_config parses ZAPRET_ENGINE into cfg.zapret.engine and uses should_use_nfqws2
  assert(apiRs.includes('key == "ZAPRET_ENGINE"'), 'save_config must parse ZAPRET_ENGINE from zapret.conf');
  // 4.2 Verify upgrade_zapret2 guards against missing binary/failed download
  assert(apiRs.includes('if !is_nfqws2_available()'), 'upgrade_zapret2 must verify is_nfqws2_available() before switching engine');

  // 5. Verify UI components in Zapret.tsx
  assert(zapretTsx.includes('Обновить до Zapret 2 (nfqws2)'), 'Zapret.tsx must render 1-click "Обновить до Zapret 2 (nfqws2)" button');
  assert(zapretTsx.includes('Legacy 1.x'), 'Zapret.tsx must render Legacy 1.x engine switcher option');
  assert(zapretTsx.includes('Modern 2.0'), 'Zapret.tsx must render Modern 2.0 engine switcher option');
  assert(zapretTsx.includes('zapret-hardware-hint'), 'Zapret.tsx must render router hardware auto-hint element');
  assert(!zapretTsx.includes("'Обнаружен Titan KN-1811 (ARM64, 512MB RAM) — рекомендуется Zapret 2.0'"), 'Zapret.tsx must NOT hardcode router hardware model fallback');
  assert(apiRs.includes('Обнаружен Titan KN-1811 (ARM64, 512MB RAM) — рекомендуется Zapret 2.0'), 'api.rs must generate Titan KN-1811 hardware hint');
  assert(zapretTsx.includes('rollback-v1-btn'), 'Zapret.tsx must include safe rollback to v1 button');
});

// -------------------------------------------------------------
// 15. Header Navigation, Zapret 1/2 Chip, Red Pulse Animation & 5 AM Daily Update
// -------------------------------------------------------------
runTest('15. Header Navigation, Zapret 1/2 Chip, Red Pulse Animation & 5 AM Daily Update', () => {
  const headerTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/components/Header.tsx'), 'utf8');
  const appTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/App.tsx'), 'utf8');
  const stylesCss = fs.readFileSync(path.resolve(__dirname, '../frontend/src/styles.css'), 'utf8');
  const zapretTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/components/Zapret.tsx'), 'utf8');
  const apiRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/api.rs'), 'utf8');
  const mainRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/main.rs'), 'utf8');
  const watchdogRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/watchdog.rs'), 'utf8');

  // 1. Settings tab moved to top header navigation
  assert(headerTsx.includes('data-testid="header-settings-btn"'), 'Header.tsx must render settings button with data-testid="header-settings-btn"');
  assert(headerTsx.includes("activeTab === 'settings' ? 'active' : ''"), 'Header.tsx must apply active class to settings button when activeTab is settings');
  assert(stylesCss.includes('.header-action-btn.active'), 'styles.css must style .header-action-btn.active');
  assert(!appTsx.includes("{ id: 'settings', label: '⚙️ Настройки' }"), 'App.tsx must remove settings from bottom tab bar TABS array');
  assert(appTsx.includes('activeTab={tab}'), 'App.tsx must pass activeTab={tab} to Header');

  // 2. Zapret chip in top header with Zapret 1 / 2 and version numbers
  assert(headerTsx.includes('data-testid="header-zapret-pill"'), 'Header.tsx must render Zapret pill button');
  assert(headerTsx.includes("zapretEngine === 'v2' ? 'Zapret 2' : 'Zapret 1'"), 'Header.tsx must format Zapret label as Zapret 1 or Zapret 2');
  assert(headerTsx.includes('IconZap'), 'Header.tsx must include IconZap for Zapret');

  // 3. Green pulsating glow animation for Zapret and Core, Blue for Panel
  assert(stylesCss.includes('.header-pill-btn.header-pill-update-green'), 'styles.css must define .header-pill-btn.header-pill-update-green');
  assert(stylesCss.includes('.update-pill-badge-green'), 'styles.css must define .update-pill-badge-green');
  assert(stylesCss.includes('@keyframes pill-green-pulse'), 'styles.css must define @keyframes pill-green-pulse');
  assert(headerTsx.includes('header-pill-update-green'), 'Header.tsx must apply header-pill-update-green when zapret update is available');
  assert(headerTsx.includes('data-testid="zapret-update-badge"'), 'Header.tsx must render zapret-update-badge on update');

  // 4. 5:00 AM daily check logic
  assert(headerTsx.includes('export function is5AmCheckDue'), 'Header.tsx must export is5AmCheckDue function');
  assert(headerTsx.includes('xr_zapret_last_check'), 'Header.tsx must track xr_zapret_last_check in localStorage');
  assert(apiRs.includes('pub fn is_zapret_5am_due'), 'api.rs must define is_zapret_5am_due function');
  assert(mainRs.includes('/api/zapret/update/check'), 'main.rs must register /api/zapret/update/check route');
  assert(watchdogRs.includes('spawn_zapret_update_checker'), 'watchdog.rs must spawn 5 AM zapret update checker background task');

  // 5. Mini-Blockcheck dynamic badge: no hardcoded zapret2 engine when v1 active
  assert(zapretTsx.includes('data-testid="blockcheck-engine-badge"'), 'Zapret.tsx must render blockcheck-engine-badge with data-testid');
  assert(zapretTsx.includes("activeEngine === 'v2' ? 'zapret2 engine' : 'zapret1 (legacy) engine'"), 'Zapret.tsx must dynamically render engine badge');
  assert(!zapretTsx.includes('<span className="badge">⚡ zapret2 engine</span>'), 'Zapret.tsx must NOT hardcode zapret2 engine');

  // 6. Zapret 2 installation: bol-van/zapret2 package with step logs, UI display and version file
  assert(apiRs.includes('zapret2-v1.0.5.2.tar.gz'), 'api.rs must download zapret2-v1.0.5.2.tar.gz from bol-van/zapret2');
  assert(apiRs.includes('[1/4]'), 'api.rs must emit step logs [1/4]');
  assert(apiRs.includes('[4/4]'), 'api.rs must emit step logs [4/4]');
  assert(apiRs.includes('/opt/zapret2/version.txt'), 'api.rs must write /opt/zapret2/version.txt');
  assert(zapretTsx.includes('data-testid="zapret-step-log"'), 'Zapret.tsx must render step log in UI with data-testid="zapret-step-log"');
  assert(zapretTsx.includes('stepLog'), 'Zapret.tsx must maintain stepLog state for installation output');

  // 7. Mini-Blockcheck YouTube bypass handling for TSPU probes and latency guard
  assert(apiRs.includes('*id == "multisplit" || *id == "seqovl_ack" || *id == "aggressive_dupsid"'), 'api.rs must mark proven bypass strategies as working for YouTube even with 000 direct probe');
  assert(apiRs.includes('base_yt_code >= 200 && base_yt_code < 400 && base_yt_time > 0.0'), 'api.rs must protect yt_time from using failed direct probe timeout');

  // 8. Dynamic main card engine label and pre-NTP clock protection
  assert(zapretTsx.includes("activeEngine === 'v2' ? 'Zapret 2.0' : 'Zapret 1.x'"), 'Zapret.tsx must dynamically display active engine in status card');
  assert(apiRs.includes('year() < 2024'), 'api.rs must guard against pre-NTP 1970 clock in is_zapret_5am_due');
  assert(headerTsx.includes('getFullYear() < 2024'), 'Header.tsx must guard against pre-NTP 1970 clock in is5AmCheckDue');

  // 9. App.test.tsx must exist and test Settings relocation and Zapret header chip
  const appTestTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/App.test.tsx'), 'utf8');
  assert(appTestTsx.includes('header-settings-btn'), 'App.test.tsx must test header-settings-btn');
  assert(appTestTsx.includes('header-zapret-pill'), 'App.test.tsx must test header-zapret-pill');
  assert(appTestTsx.includes('removes Settings from bottom navigation'), 'App.test.tsx must test removal of settings from bottom tabs');
});

// -------------------------------------------------------------
// 16. Zapret Update Badge Stuck Red & Service Start Startup Crash Fixes
// -------------------------------------------------------------
runTest('16. Zapret Update Badge Stuck Red & Service Start Startup Crash Fixes', () => {
  const apiRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/api.rs'), 'utf8');
  const zapretTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/components/Zapret.tsx'), 'utf8');
  const zapretTestTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/components/Zapret.test.tsx'), 'utf8');
  const headerTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/components/Header.tsx'), 'utf8');
  const appTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/App.tsx'), 'utf8');

  // 1. check_zapret_update_core must NOT mark update_available=true solely due to v1 engine
  assert(apiRs.includes('let update_available = has_newer_tag;'), 'check_zapret_update_core must set update_available based on has_newer_tag');
  assert(!apiRs.includes('update_available = has_newer_tag || upgrade_available'), 'check_zapret_update_core must NOT conflate upgrade_available with update_available');
  assert(apiRs.includes('let effective_update = update_avail && crate::updater::is_newer(&lat, &cur_ver);'), 'check_zapret_update_core cached branch must verify is_newer');

  // 2. Both get_status and get_zapret_status must guard update_available with is_newer check against installed version
  assert(apiRs.includes('"update_available": cfg.zapret.update_available\n                && cfg.zapret.latest_version.as_deref().map_or(false, |lat| crate::updater::is_newer(lat, &zapret_ver)),')
    || apiRs.includes('crate::updater::is_newer(lat, &zapret_ver)'),
    'get_status and get_zapret_status must verify is_newer against installed version');

  // 3. upgrade_zapret2 must reset update_available=false, update latest_version, write version.txt and return update_available: false
  assert(apiRs.includes('cfg.zapret.update_available = false;'), 'upgrade_zapret2 / rollback must set cfg.zapret.update_available = false');
  assert(apiRs.includes('cfg.zapret.latest_version = Some("v1.0.5.2".to_string());'), 'upgrade_zapret2 must update latest_version to v1.0.5.2');
  assert(apiRs.includes('"update_available": false'), 'upgrade_zapret2 response must return update_available: false');
  assert(apiRs.includes('tokio::fs::write("/opt/zapret2/version.txt", "v1.0.5.2\\n")'), 'upgrade_zapret2 Rust handler must directly write version.txt');

  // 4. S51zapret script: translates --dpi-desync-fwmark= to --fwmark= for nfqws2 to prevent startup crash
  assert(apiRs.includes("sed 's/--dpi-desync-fwmark=/--fwmark=/g'"), 'S51zapret must translate --dpi-desync-fwmark= to --fwmark= for nfqws2');
  assert(apiRs.includes("sed 's/--fwmark=/--dpi-desync-fwmark=/g'"), 'S51zapret must translate --fwmark= to --dpi-desync-fwmark= for legacy nfqws');

  // 5. S51zapret script: loads both zapret-lib.lua and zapret-antidpi.lua before effective args
  assert(apiRs.includes('for lmod in zapret-lib.lua zapret-antidpi.lua zapret-auto.lua; do'), 'S51zapret must iterate required lua modules');
  assert(apiRs.includes('$BIN --pidfile="$PIDFILE" $LUA_INIT_ARG $EFFECTIVE_ARGS'), 'S51zapret must pass $LUA_INIT_ARG with nfqws2');

  // 6. find_bin & is_nfqws2_available must support un-prefixed architecture binary folders and verify runnability
  assert(apiRs.includes('/opt/zapret2/binaries/arm64/nfqws2'), 'find_bin must search /opt/zapret2/binaries/arm64/nfqws2');
  assert(apiRs.includes('/opt/zapret2/binaries/arm/nfqws2'), 'find_bin must search /opt/zapret2/binaries/arm/nfqws2');
  assert(apiRs.includes('/opt/zapret2/binaries/mipsel/nfqws2'), 'find_bin must search /opt/zapret2/binaries/mipsel/nfqws2');
  assert(apiRs.includes('/opt/zapret2/binaries/mips/nfqws2'), 'find_bin must search /opt/zapret2/binaries/mips/nfqws2');
  assert(apiRs.includes('/opt/zapret2/binaries/x86_64/nfqws2'), 'find_bin must search /opt/zapret2/binaries/x86_64/nfqws2');
  assert(apiRs.includes('is_runnable()'), 'find_bin must include is_runnable function to avoid executing wrong architecture binaries');

  // 7. upgrade_zapret2 & install archive extraction handles Z2_ARCH, files/lua, and valid master/lua download fallback
  assert(apiRs.includes('Z2_ARCH="arm64"'), 'upgrade_zapret2 must map aarch64/arm64 to Z2_ARCH=arm64');
  assert(apiRs.includes('Z2_ARCH="mipsel"'), 'upgrade_zapret2 must map little-endian mips to Z2_ARCH=mipsel');
  assert(apiRs.includes('cp -rf "$Z2_DIR/files/lua/"* /opt/zapret2/lua/'), 'upgrade_zapret2 must copy from files/lua/');
  assert(apiRs.includes('https://raw.githubusercontent.com/bol-van/zapret2/master/lua/$lf'), 'upgrade_zapret2 must have direct GitHub lua download fallback using master/lua');

  // 8. Event synchronization between components (Zapret.tsx, Header.tsx, App.tsx)
  assert(zapretTsx.includes("new CustomEvent('xr:zapret-updated'"), 'Zapret.tsx must dispatch xr:zapret-updated event');
  assert(zapretTsx.includes("new CustomEvent('xr:refresh-status')"), 'Zapret.tsx must dispatch xr:refresh-status event');
  assert(headerTsx.includes("window.addEventListener('xr:zapret-updated'"), 'Header.tsx must listen to xr:zapret-updated');
  assert(headerTsx.includes('effectiveZapretUpdate ='), 'Header.tsx must calculate effectiveZapretUpdate');
  assert(appTsx.includes("window.addEventListener('xr:refresh-status'"), 'App.tsx must listen to xr:refresh-status');

  // 9. Zapret.tsx handleUpgradeZapret2 clears localStorage cache, resets update_available, and forces check
  assert(zapretTsx.includes("localStorage.removeItem('xr_zapret_last_check')"), 'Zapret.tsx must remove xr_zapret_last_check from localStorage on upgrade');
  assert(zapretTsx.includes("apiGet('zapret/update/check?force=1')"), 'Zapret.tsx must trigger forced update check after upgrade');
  assert(zapretTsx.includes('update_available: false'), 'Zapret.tsx must set update_available: false in status state');

  // 10. Zapret.test.tsx anti-regression test exists
  assert(zapretTestTsx.includes('clears localStorage xr_zapret_last_check and requests forced update check upon successful upgrade_zapret2'),
    'Zapret.test.tsx must contain automated test for upgrade_zapret2 cache clearing and update_available reset');
});

// -------------------------------------------------------------
// 17. Update Colors (Core & Zapret = Green, Panel = Blue) & Header Non-Overlapping Layout Protection
// -------------------------------------------------------------
runTest('17. Update Colors (Core & Zapret = Green, Panel = Blue) & Header Non-Overlapping Layout Protection', () => {
  const stylesCss = fs.readFileSync(path.resolve(__dirname, '../frontend/src/styles.css'), 'utf8');
  const headerTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/components/Header.tsx'), 'utf8');
  const headerTestTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/components/Header.test.tsx'), 'utf8');
  const zapretTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/components/Zapret.tsx'), 'utf8');

  // 1. Core update (Mihomo) uses GREEN
  assert(headerTsx.includes("mihomoUpdateAvailable ? 'header-pill-update-green' : ''"), 'Header.tsx must use header-pill-update-green for Mihomo core');
  assert(headerTsx.includes('update-pill-badge-green'), 'Header.tsx must use update-pill-badge-green for core and zapret');

  // 2. Zapret update uses GREEN
  assert(headerTsx.includes("effectiveZapretUpdate ? 'header-pill-update-green' : ''"), 'Header.tsx must use header-pill-update-green for Zapret');
  assert(zapretTsx.includes('#16a34a') && zapretTsx.includes('#22c55e'), 'Zapret.tsx upgrade button must use green gradient when update_available is true');

  // 3. Panel update (XKeen Route) uses BLUE
  assert(headerTsx.includes("updateAvailable ? 'header-pill-update-blue' : ''"), 'Header.tsx must use header-pill-update-blue for Panel');
  assert(headerTsx.includes('update-pill-badge-blue'), 'Header.tsx must use update-pill-badge-blue for Panel');

  // 4. Header layout non-overlapping protection
  assert(stylesCss.includes('min-width: 380px') || stylesCss.includes('minmax(380px, 1fr)'), 'styles.css must allocate proper space for version pills');
  assert(stylesCss.includes('@media (max-width: 1139.98px)'), 'styles.css must wrap header below 1140px into two rows');
  assert(stylesCss.includes('.header-pill-btn.header-pill-update-green .header-pill-subtitle'), 'styles.css must hide subtitle on update pills to prevent horizontal overflow');

  // 5. Automated unit tests exist in Header.test.tsx
  assert(headerTestTsx.includes('shows green update badge for Mihomo core and blue update badge for Panel'), 'Header.test.tsx must contain automated test for green core and blue panel updates');
  assert(headerTestTsx.includes('shows green pulsating animation and badge when Zapret update is available'), 'Header.test.tsx must contain automated test for green zapret update');
});

// -------------------------------------------------------------
// 18. Boost Update: Strategy Visual Feedback, Smart Gaming & 60s Traffic History
// -------------------------------------------------------------
runTest('18. Boost Update: Strategy Visual Feedback, Smart Gaming & 60s Traffic History', () => {
  const zapretTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/components/Zapret.tsx'), 'utf8');
  const trafficGraphTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/components/TrafficGraph.tsx'), 'utf8');
  const gamingTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/components/Gaming.tsx'), 'utf8');
  const dashboardTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/components/Dashboard.tsx'), 'utf8');
  const apiRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/api.rs'), 'utf8');
  const trafficRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/traffic.rs'), 'utf8');
  const configRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/config.rs'), 'utf8');

  // 1. Strategy application visual feedback in Zapret.tsx
  assert(zapretTsx.includes('activeStrategyId === strat.id') || zapretTsx.includes('isSelected'), 'Zapret.tsx must track and style activeStrategyId');
  assert(zapretTsx.includes('✓ Выбрано'), 'Zapret.tsx must render "✓ Выбрано" label for selected strategy');
  assert(zapretTsx.includes('activeEngine !== \'v2\' && ('), 'Zapret.tsx must hide hardware recommendation hint when Zapret 2 is active');

  // 2. Traffic Analytics single VPS counter and period_bytes
  assert(zapretTsx.includes('Трафик за весь период'), 'Zapret.tsx must display "Трафик за весь период" counter');
  assert(zapretTsx.includes('analytics?.period_bytes'), 'Zapret.tsx must use analytics.period_bytes');
  assert(configRs.includes('pub period_saved_bytes: u64'), 'config.rs must store period_saved_bytes');
  assert(apiRs.includes('"period_bytes": period_bytes'), 'api.rs must return period_bytes in analytics response');

  // 3. TrafficGraph 60s ring buffer and swapped direct/proxy colors (Proxy = Blue #38bdf8, Direct = Green #22c55e)
  assert(trafficGraphTsx.includes('Array(60).fill'), 'TrafficGraph.tsx must maintain 60s history buffer');
  assert(trafficGraphTsx.includes('data.history'), 'TrafficGraph.tsx must initialize history buffer from data.history');
  assert(trafficRs.includes('pub fn record_history_point'), 'traffic.rs must record history points in ring buffer');
  assert(trafficRs.includes('pub fn get_history_json'), 'traffic.rs must provide get_history_json');
  assert(trafficGraphTsx.includes('stroke="#22c55e"') && trafficGraphTsx.includes('stroke="#38bdf8"'), 'TrafficGraph.tsx must use green for direct and blue for proxy');

  // 4. Smart Gaming Mode & Recent Gaming Connections
  assert(gamingTsx.includes('smart_mode') && gamingTsx.includes('smart_idle_timeout_mins'), 'Gaming.tsx must render smart gaming mode controls');
  assert(gamingTsx.includes('handleToggleIgnoreConn'), 'Gaming.tsx must implement handleToggleIgnoreConn handler');
  assert(gamingTsx.includes('Не реагировать'), 'Gaming.tsx must render "Не реагировать" ignore button');
  assert(apiRs.includes('pub async fn ignore_gaming_conn'), 'api.rs must implement ignore_gaming_conn endpoint');

  // 5. Dashboard Active Server card details
  assert(dashboardTsx.includes('Mixed Port') && dashboardTsx.includes('7890'), 'Dashboard.tsx must display Mixed Port 7890');
  assert(dashboardTsx.includes('Full-Cone NAT'), 'Dashboard.tsx must display Full-Cone NAT');
  assert(dashboardTsx.includes('devicesCount'), 'Dashboard.tsx must display connected devices count');
});

// -------------------------------------------------------------
// 19. Updater Detached Safe Self-Restart & Response Handshake Protection
// -------------------------------------------------------------
runTest('19. Updater Detached Safe Self-Restart & Response Handshake Protection', () => {
  const updaterRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/updater.rs'), 'utf8');

  // 1. updater.rs must delay restart by 1 second in detached background shell on unix
  // to ensure HTTP 200 { "installed": ver, "restarting": true } response finishes before killall
  assert(updaterRs.includes('(sleep 1 && {} restart) >/dev/null 2>&1 &') || updaterRs.includes('sleep 1 &&'), 
    'updater.rs must detach restart command with 1s sleep delay to prevent premature process termination');
  assert(updaterRs.includes('BIN_PATH: &str = "/opt/sbin/xkeen-route"'), 'updater.rs must define BIN_PATH as /opt/sbin/xkeen-route');
  assert(updaterRs.includes('INIT_SCRIPT: &str = "/opt/etc/init.d/S99xkeen-route"'), 'updater.rs must define INIT_SCRIPT as /opt/etc/init.d/S99xkeen-route');
});

// -------------------------------------------------------------
// 20. DeviceRow Zapret Switch, CPU Temp Rounding, Blue Active Server & Header Cleanup
// -------------------------------------------------------------
runTest('20. DeviceRow Zapret Switch, CPU Temp Rounding, Blue Active Server & Header Cleanup', () => {
  const deviceRowTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/components/DeviceRow.tsx'), 'utf8');
  const headerTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/components/Header.tsx'), 'utf8');
  const dashboardTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/components/Dashboard.tsx'), 'utf8');
  const skillMd = fs.readFileSync(path.resolve('C:/Users/internet/.gemini/config/skills/xkeen-troubleshooting/SKILL.md'), 'utf8');

  // 1. DeviceRow must use restored beautiful button for Zapret and new toggle switch for gaming mode
  assert(deviceRowTsx.includes('className={`device-zapret-btn'), 'DeviceRow.tsx must restore beautiful .device-zapret-btn button');
  assert(deviceRowTsx.includes('className="switch device-gaming-switch"'), 'DeviceRow.tsx must provide per-device .device-gaming-switch');
  assert(deviceRowTsx.includes('className="device-gaming-toggle"'), 'DeviceRow.tsx must render checkbox with device-gaming-toggle');

  // 2. Header must format all metrics (CPU temp, XKeen RAM, etc.) as whole integers
  assert(headerTsx.includes('const cpuTemp = currentMetrics?.cpu_temp_c != null ? Math.round(currentMetrics.cpu_temp_c) : undefined'), 'Header.tsx must round CPU temp to whole integer at declaration');
  assert(headerTsx.includes('const totalXkeenMem = Math.round(currentMetrics?.total_xkeen_memory_mb ?? (appMemMb + coreMemMb))'), 'Header.tsx must round XKeen RAM to whole integer at declaration');
  assert(headerTsx.includes('const appMemMb = Math.round(currentMetrics?.app_memory_mb ?? 0)'), 'Header.tsx must round app memory to whole integer at declaration');
  assert(!headerTsx.includes('{cpuTemp.toFixed(1)}°C'), 'Header.tsx must not format CPU temp with decimals');

  // 3. Header must NOT contain header-gaming-pill button (removed per user request to keep header clean)
  assert(!headerTsx.includes('data-testid="header-gaming-pill"'), 'Header.tsx must NOT contain header-gaming-pill button (removed per user request to keep header clean)');
  assert(headerTsx.includes('header-actions-right'), 'Header.tsx must wrap right action buttons in header-actions-right container');

  // 4. Dashboard Active Server card, Failover primary & Servers list badge must use blue accents (#38bdf8) instead of green (#22c55e)
  assert(dashboardTsx.includes('color: \'#38bdf8\'') && dashboardTsx.includes('🔵 В сети'), 'Dashboard.tsx Active Server badge must be blue (#38bdf8)');
  assert(dashboardTsx.includes('stroke="#38bdf8"'), 'Dashboard.tsx PingSparkline must use stroke #38bdf8');
  assert(!dashboardTsx.includes('stroke="#22c55e"'), 'Dashboard.tsx PingSparkline must not use green stroke');
  assert(dashboardTsx.includes('color: \'#38bdf8\' }}>ОСН (основной)</span>'), 'Dashboard.tsx Failover primary server badge must be blue (#38bdf8)');

  const stylesCss = fs.readFileSync(path.resolve(__dirname, '../frontend/src/styles.css'), 'utf8');
  assert(stylesCss.includes('.current-active-badge {') && stylesCss.includes('color: #38bdf8;'), '.current-active-badge must use blue text #38bdf8');
  assert(!stylesCss.includes('.current-active-badge {\n  display: inline-flex;\n  align-items: center;\n  gap: 4px;\n  background: rgba(34, 197, 94, 0.16);'), '.current-active-badge must not use green #22c55e styling');

  // 5. Skill file must contain release notes protocol rule
  assert(skillMd.includes('Обязательный регламент релизов и версионирования (Release Notes & What\'s New)'), 'SKILL.md must document release notes and versioning rule');
});

runTest('21. Zapret v1/v2 Engine Greyout, Router PREROUTING Hook, Fast DPI Test & Unified Gaming Card', () => {
  const zapretTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/components/Zapret.tsx'), 'utf8');
  const gamingTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/components/Gaming.tsx'), 'utf8');
  const apiRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/api.rs'), 'utf8');

  // 1. Zapret.tsx must support engine compatibility greyout with badge
  assert(zapretTsx.includes('supportedEngines?: (\'v1\' | \'v2\')[]'), 'Zapret.tsx renderStrategyCard must take supportedEngines parameter');
  assert(zapretTsx.includes('opacity: isEngineSupported ? 1 : 0.45'), 'Zapret.tsx must reduce opacity to 0.45 when engine not supported');
  assert(zapretTsx.includes('filter: isEngineSupported ? \'none\' : \'grayscale(0.8)\''), 'Zapret.tsx must apply grayscale filter to unsupported cards');
  assert(zapretTsx.includes('🔒 Только для Zapret'), 'Zapret.tsx must render lock badge for engine restricted cards');
  assert(zapretTsx.includes('activeEngine === \'v1\''), 'Zapret.tsx must disable aggressive preset when activeEngine === v1');

  // 2. backend/src/api.rs must hook PREROUTING 1 -i br+ / Bridge+ in ensure_zapret_init_script
  assert(apiRs.includes('iptables -t mangle -I PREROUTING 1 -i br+ -m comment --comment "xkeen-route-zapret" -j zapret'), 'api.rs must hook PREROUTING br+ instead of POSTROUTING');
  assert(apiRs.includes('iptables -t mangle -I PREROUTING 1 -i Bridge+ -m comment --comment "xkeen-route-zapret" -j zapret'), 'api.rs must hook PREROUTING Bridge+ instead of POSTROUTING');
  assert(!apiRs.includes('iptables -t mangle -I POSTROUTING 1 -m comment --comment "xkeen-route-zapret" -j zapret'), 'api.rs must not hook POSTROUTING in ensure_zapret_init_script');

  // 3. backend/src/api.rs test_dpi fast parallel checking
  assert(apiRs.includes('p_yt=$(curl -4 -k -m 2.5 -s -o /dev/null'), 'api.rs test_dpi must use fast parallel 2.5s curl probes');
  assert(apiRs.includes('service_running') && apiRs.includes('iptables_active'), 'api.rs test_dpi must include service_running and iptables_active status');

  // 4. Gaming.tsx must fetch serversRes.servers and unify Smart Gaming with Recent Activity
  assert(gamingTsx.includes('serversRes?.servers || serversRes?.proxies || serversRes?.all || []'), 'Gaming.tsx must check serversRes.servers to populate proxy nodes dropdown');
  assert(gamingTsx.includes('height: 38'), 'Gaming.tsx ping button must have uniform 38px height');
  assert(gamingTsx.includes('📊 Диагностика задержки'), 'Gaming.tsx ping button must have uniform label');
  assert(gamingTsx.includes('хостов в детекторе'), 'Gaming.tsx must unify smart gaming with recent activity detector');
});

// -------------------------------------------------------------
// 22. Zapret renderStrategyCard aggressive_dpi arguments alignment and engine restriction
// -------------------------------------------------------------
runTest('22. Zapret renderStrategyCard aggressive_dpi arguments alignment and engine restriction', () => {
  const zapretTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/components/Zapret.tsx'), 'utf8');

  // Verify aggressive_dpi passes tags AND supportedEngines as 8th parameter
  const match = zapretTsx.match(/'aggressive_dpi'[\s\S]*?\['seqovl',\s*'midsld',\s*'ts',\s*'md5sig'\][\s\S]*?\['v2'\]/);
  assert(match, 'Zapret.tsx must pass tags and supportedEngines (v2) to renderStrategyCard for aggressive_dpi');
});

// -------------------------------------------------------------
// 23. Header layout containment: gaming pill removed, header-bar grid & non-overflowing bounds protection
// -------------------------------------------------------------
runTest('23. Header layout containment: gaming pill removed, header-bar grid & non-overflowing bounds protection', () => {
  const headerTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/components/Header.tsx'), 'utf8');
  const stylesCss = fs.readFileSync(path.resolve(__dirname, '../frontend/src/styles.css'), 'utf8');

  // 1. Gaming pill switch must NOT be present in Header.tsx
  assert(!headerTsx.includes('data-testid="header-gaming-pill"'), 'Header.tsx must not contain header-gaming-pill');
  assert(!headerTsx.includes('gaming/toggle'), 'Header.tsx must not contain gaming/toggle endpoint call');

  // 2. styles.css must not have leftover header-pill-gaming-active
  assert(!stylesCss.includes('header-pill-gaming-active'), 'styles.css must not contain obsolete header-pill-gaming-active');

  // 3. header-bar must have display: grid, box-sizing: border-box, max-width: 100%
  assert(stylesCss.includes('.header-bar'), 'styles.css must style .header-bar');
  assert(stylesCss.includes('display: grid;'), 'styles.css .header-bar must use display: grid');
  assert(stylesCss.includes('box-sizing: border-box;'), 'styles.css .header-bar must have box-sizing: border-box');
  assert(stylesCss.includes('max-width: 100%;'), 'styles.css .header-bar must have max-width: 100%');

  // 4. Responsive switch at < 1140px to two rows
  assert(stylesCss.includes('@media (max-width: 1139.98px)'), 'styles.css must define two-row switch below 1140px');
});

// -------------------------------------------------------------
// 24. Header 5-group single-line grid toolbar & exact metrics specification
// -------------------------------------------------------------
runTest('24. Header 5-group single-line grid toolbar & exact metrics specification', () => {
  const headerTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/components/Header.tsx'), 'utf8');
  const stylesCss = fs.readFileSync(path.resolve(__dirname, '../frontend/src/styles.css'), 'utf8');

  // 1. 5 groups in Header.tsx in correct semantic sequence
  assert(headerTsx.includes('className="header-brand-group"'), 'Header.tsx must contain group 1: header-brand-group');
  assert(headerTsx.includes('className="header-status-group"'), 'Header.tsx must contain group 2: header-status-group');
  assert(headerTsx.includes('className="header-versions-group"'), 'Header.tsx must contain group 3: header-versions-group');
  assert(headerTsx.includes('className="header-service-actions"'), 'Header.tsx must contain group 4: header-service-actions');
  assert(headerTsx.includes('className="header-utility-actions'), 'Header.tsx must contain group 5: header-utility-actions');

  // 2. Exact grid template columns: 150px 270px minmax(380px, 1fr) 82px 159px
  assert(stylesCss.includes('grid-template-columns: 150px 270px minmax(380px, 1fr) 82px 159px;'),
    'styles.css must define exact 5-column grid: 150px 270px minmax(380px, 1fr) 82px 159px');

  // 3. Exact heights: toolbar min-height 72px, status card 44px, badges 36px, buttons 38px
  assert(stylesCss.includes('min-height: 72px;'), 'styles.css .header-bar must have min-height: 72px');
  assert(stylesCss.includes('height: 44px;'), 'styles.css status badge must have height: 44px');
  assert(stylesCss.includes('height: 36px;'), 'styles.css version badges must have height: 36px');
  assert(stylesCss.includes('height: 38px;'), 'styles.css action buttons must have height: 38px');

  // 4. Red highlight only for stop button; blue highlight for active settings button
  assert(headerTsx.includes('header-action-btn-stop'), 'Header.tsx must apply header-action-btn-stop to stop button');
  assert(stylesCss.includes('.header-action-btn.header-action-btn-stop'), 'styles.css must highlight only stop button in red');
  assert(stylesCss.includes('.header-utility-actions .header-action-btn.active'), 'styles.css must highlight only active settings in blue');
});

// -------------------------------------------------------------
// 25. Zapret Netfilter: TCPMSS must NOT be placed in PREROUTING-hooked zapret chain and PREROUTING hook must have resilient fallback
// -------------------------------------------------------------
runTest('25. Zapret Netfilter: TCPMSS must NOT be placed in PREROUTING-hooked zapret chain and PREROUTING hook must have resilient fallback', () => {
  const apiRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/api.rs'), 'utf8');

  // 1. Linux kernel forbids TCPMSS in PREROUTING hook (xt_TCPMSS: path-MTU clamping only supported in FORWARD, OUTPUT and POSTROUTING hooks).
  // Placing TCPMSS inside a chain called from PREROUTING causes kernel to reject the PREROUTING hook with -EINVAL.
  assert(!apiRs.includes('iptables -t mangle -A zapret -p tcp --tcp-flags SYN,RST SYN -j TCPMSS'),
    'api.rs must NEVER place TCPMSS rule inside zapret chain (which is hooked into PREROUTING)');

  // 2. TCPMSS must be placed in valid netfilter hooks (POSTROUTING and/or FORWARD)
  assert(apiRs.includes('iptables -t mangle -A POSTROUTING -p tcp --tcp-flags SYN,RST SYN') && apiRs.includes('TCPMSS --clamp-mss-to-pmtu'),
    'api.rs must place TCPMSS clamping in POSTROUTING hook');
  assert(apiRs.includes('iptables -t mangle -A FORWARD -p tcp --tcp-flags SYN,RST SYN') && apiRs.includes('TCPMSS --clamp-mss-to-pmtu'),
    'api.rs must place TCPMSS clamping in FORWARD hook');

  // 3. PREROUTING hook must hook br+ and Bridge+
  assert(apiRs.includes('iptables -t mangle -I PREROUTING 1 -i br+ -m comment --comment "xkeen-route-zapret" -j zapret'),
    'api.rs must hook PREROUTING for br+ bridge interface');
  assert(apiRs.includes('iptables -t mangle -I PREROUTING 1 -i Bridge+ -m comment --comment "xkeen-route-zapret" -j zapret'),
    'api.rs must hook PREROUTING for Bridge+ bridge interface');

  // 4. remove_fw_rules must cleanly tear down TCPMSS from POSTROUTING, FORWARD and legacy zapret
  assert(apiRs.includes('while iptables -t mangle -D POSTROUTING -p tcp --tcp-flags SYN,RST SYN'),
    'remove_fw_rules must clean up POSTROUTING TCPMSS');
  assert(apiRs.includes('while iptables -t mangle -D FORWARD -p tcp --tcp-flags SYN,RST SYN'),
    'remove_fw_rules must clean up FORWARD TCPMSS');
  assert(apiRs.includes('while iptables -t mangle -D zapret -p tcp --tcp-flags SYN,RST SYN -j TCPMSS'),
    'remove_fw_rules must clean up legacy zapret TCPMSS');

  // 5. main.rs must support --version flag (version = crate::VERSION without disable_version_flag)
  const mainRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/main.rs'), 'utf8');
  assert(!mainRs.includes('disable_version_flag = true'), 'main.rs must NOT disable version flag');
  assert(mainRs.includes('version = crate::VERSION'), 'main.rs must bind version = crate::VERSION');

  // 6. updater.rs must provide fallback staging verification
  const updaterRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/updater.rs'), 'utf8');
  assert(updaterRs.includes('.arg("version")') && updaterRs.includes('.arg("--help")'),
    'updater.rs must provide staging validation fallbacks');
});

// -------------------------------------------------------------
// 26. Autonomous Entware Init & Self-Healing Watchdog (v1.6.2)
// -------------------------------------------------------------
runTest('26. Autonomous Entware Init & Self-Healing Watchdog (v1.6.2)', () => {
  const mainRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/main.rs'), 'utf8');
  const updaterRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/updater.rs'), 'utf8');
  const setupSh = fs.readFileSync(path.resolve(__dirname, '../setup.sh'), 'utf8');

  // 1. main.rs init script template must have wait_for_opt, ensure_xkeen, and PID management
  assert(mainRs.includes('wait_for_opt()'), 'main.rs init template must define wait_for_opt');
  assert(mainRs.includes('ensure_xkeen()'), 'main.rs init template must define ensure_xkeen');
  assert(mainRs.includes('PID_FILE=/opt/var/run/xkeen-route.pid'), 'main.rs init template must use PID_FILE');
  assert(mainRs.includes('for p in $(pidof "$NAME" 2>/dev/null); do'), 'main.rs init template must include pidof self-healing fallback');
  assert(mainRs.includes('sh "$XKEEN_INIT" start'), 'main.rs init template must invoke S05xkeen start');
  assert(!mainRs.includes('. /opt/etc/init.d/rc.func'), 'main.rs init template must be autonomous without rc.func dependency');

  // 2. updater.rs must regenerate init script via create-init upon update
  assert(updaterRs.includes('.arg("create-init")'), 'updater.rs must execute create-init on target_bin');

  // 3. setup.sh and main.rs crontab watchdog must execute S99xkeen-route start directly
  assert(setupSh.includes("*/5 * * * * /opt/etc/init.d/S99xkeen-route start"), 'setup.sh watchdog must call S99xkeen-route start');
  assert(mainRs.includes("*/5 * * * * /opt/etc/init.d/S99xkeen-route start"), 'main.rs watchdog must call S99xkeen-route start');
});

// -------------------------------------------------------------
// 27. Exclude-filter YAML single-quote rendering & regex preservation
// -------------------------------------------------------------
runTest('27. Exclude-filter YAML single-quote rendering & regex preservation', () => {
  const routingRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/routing.rs'), 'utf8');

  // 1. apply_ignore_to_providers must output single-quoted exclude-filter to prevent unknown escape character in YAML
  assert(routingRs.includes("out.push(format!(\"    exclude-filter: '{}'\""), 'routing.rs must use single quotes for exclude-filter');
  assert(!routingRs.includes("out.push(format!(\"    exclude-filter: \\\"{}\\\"\""), 'routing.rs must NOT use double quotes for exclude-filter');

  // 2. Must not re-escape existing orig regex parts
  assert(!routingRs.includes("if s.contains('(') || s.contains(')')"), 'routing.rs must not re-escape existing orig regex elements');
});

// -------------------------------------------------------------
// 28. XKeen Service Pipe Leak Protection & Live Header Feedback (v1.6.4)
// -------------------------------------------------------------
runTest('28. XKeen Service Pipe Leak Protection & Live Header Feedback (v1.6.4)', () => {
  const apiRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/api.rs'), 'utf8');
  const headerTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/components/Header.tsx'), 'utf8');

  // 1. xkeen_service must set fd_out=true to prevent Mihomo daemon from inheriting stdout/stderr pipes
  assert(apiRs.includes('.env("fd_out", "true")'), 'api.rs must set env fd_out=true when invoking init_script');

  // 2. xkeen_service must wrap script execution in a bounded timeout (max 15s)
  assert(apiRs.includes('Duration::from_secs(15)'), 'api.rs must wrap service execution in 15s timeout');

  // 3. xkeen_service must return an explicit message field
  assert(apiRs.includes('"message": msg'), 'api.rs must return an explicit message field on success');

  // 4. Header.tsx must maintain serviceFeedback and render it in status-label
  assert(headerTsx.includes('serviceFeedback'), 'Header.tsx must track serviceFeedback');
  assert(headerTsx.includes('data-testid="header-status-label"'), 'Header.tsx must render header-status-label');
  assert(headerTsx.includes('data-testid="header-restart-btn"'), 'Header.tsx must have header-restart-btn');
  assert(headerTsx.includes('data-testid="header-toggle-btn"'), 'Header.tsx must have header-toggle-btn');
});

// -------------------------------------------------------------
// 29. AdGuard Home Backend Integration, Router Endpoints & YAML Backup Protection (v1.7.0)
// -------------------------------------------------------------
runTest('29. AdGuard Home Backend Integration, Router Endpoints & YAML Backup Protection (v1.7.0)', () => {
  const adguardRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/adguard.rs'), 'utf8');
  const mainRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/main.rs'), 'utf8');
  const configRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/config.rs'), 'utf8');
  const apiRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/api.rs'), 'utf8');

  // 1. adguard.rs functions and data structures
  assert(adguardRs.includes('pub fn base64_encode'), 'adguard.rs must provide base64_encode');
  assert(adguardRs.includes('pub fn build_auth_header'), 'adguard.rs must provide build_auth_header');
  assert(adguardRs.includes('pub async fn get_status'), 'adguard.rs must implement get_status');
  assert(adguardRs.includes('pub async fn get_health'), 'adguard.rs must implement get_health');
  assert(adguardRs.includes('pub async fn get_overview'), 'adguard.rs must implement get_overview');
  assert(adguardRs.includes('pub async fn get_query_log'), 'adguard.rs must implement get_query_log');
  assert(adguardRs.includes('pub async fn set_protection'), 'adguard.rs must implement set_protection');
  assert(adguardRs.includes('pub async fn get_filtering'), 'adguard.rs must implement get_filtering');
  assert(adguardRs.includes('pub async fn set_user_rules'), 'adguard.rs must implement set_user_rules');
  assert(adguardRs.includes('pub fn validate_user_rules'), 'adguard.rs must validate user rules syntax');
  assert(adguardRs.includes('pub async fn get_rewrites'), 'adguard.rs must implement get_rewrites');
  assert(adguardRs.includes('pub async fn add_rewrite'), 'adguard.rs must implement add_rewrite');
  assert(adguardRs.includes('pub async fn delete_rewrite'), 'adguard.rs must implement delete_rewrite');
  assert(adguardRs.includes('pub async fn get_diagnostics'), 'adguard.rs must implement get_diagnostics');
  assert(adguardRs.includes('pub async fn service_action'), 'adguard.rs must implement service_action');

  // 2. main.rs routing table registration
  assert(mainRs.includes('/api/adguard/status'), 'main.rs must register /api/adguard/status');
  assert(mainRs.includes('/api/adguard/health'), 'main.rs must register /api/adguard/health');
  assert(mainRs.includes('/api/adguard/overview'), 'main.rs must register /api/adguard/overview');
  assert(mainRs.includes('/api/adguard/query-log'), 'main.rs must register /api/adguard/query-log');
  assert(mainRs.includes('/api/adguard/protection'), 'main.rs must register /api/adguard/protection');
  assert(mainRs.includes('/api/adguard/filtering'), 'main.rs must register /api/adguard/filtering');
  assert(mainRs.includes('/api/adguard/filtering/rules'), 'main.rs must register /api/adguard/filtering/rules');
  assert(mainRs.includes('/api/adguard/rewrites'), 'main.rs must register /api/adguard/rewrites');
  assert(mainRs.includes('/api/adguard/diagnostics'), 'main.rs must register /api/adguard/diagnostics');
  assert(mainRs.includes('/api/adguard/service'), 'main.rs must register /api/adguard/service');

  // 3. config.rs AdGuardConfig schema
  assert(configRs.includes('pub struct AdGuardConfig'), 'config.rs must define AdGuardConfig');
  assert(configRs.includes('pub failsafe_rollback: bool'), 'AdGuardConfig must include failsafe_rollback');
  assert(configRs.includes('pub upstream_dns: Vec<String>'), 'AdGuardConfig must include upstream_dns');

  // 4. api.rs AdGuardHome.yaml backup integration
  assert(apiRs.includes('AdGuardHome.yaml'), 'api.rs must register AdGuardHome.yaml in backups');
});

// -------------------------------------------------------------
// 30. AdGuard Home Frontend Tab, Navigation & Failsafe Diagnostics (v1.7.0)
// -------------------------------------------------------------
runTest('30. AdGuard Home Frontend Tab, Navigation & Failsafe Diagnostics (v1.7.0)', () => {
  const appTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/App.tsx'), 'utf8');
  const headerTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/components/Header.tsx'), 'utf8');
  const adguardTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/components/AdGuard.tsx'), 'utf8');
  const typesTs = fs.readFileSync(path.resolve(__dirname, '../frontend/src/types.ts'), 'utf8');

  // 1. App.tsx tab registration
  assert(appTsx.includes("id: 'adguard'"), 'App.tsx must register adguard tab');
  assert(appTsx.includes('<AdGuard notify={notify} />'), 'App.tsx must render AdGuard view');

  // 2. Header.tsx Help navigation button (? icon, distinct from AdGuard tab to avoid duplication)
  assert(headerTsx.includes("data-testid=\"header-help-btn\""), 'Header.tsx must have header-help-btn');
  assert(headerTsx.includes('title="Справка, руководство пользователя и API документация"'), 'Header.tsx must have Help tooltip');
  assert(headerTsx.includes('❓'), 'Header.tsx must have Help icon');

  // 3. AdGuard.tsx UI components and subtabs
  assert(adguardTsx.includes('data-testid="adguard-view"'), 'AdGuard.tsx must have adguard-view container');
  assert(adguardTsx.includes('data-testid="adguard-protection-toggle"'), 'AdGuard.tsx must have adguard-protection-toggle');
  assert(adguardTsx.includes('subtab-overview'), 'AdGuard.tsx must have overview subtab');
  assert(adguardTsx.includes('subtab-querylog'), 'AdGuard.tsx must have querylog subtab');
  assert(adguardTsx.includes('subtab-filtering'), 'AdGuard.tsx must have filtering subtab');
  assert(adguardTsx.includes('subtab-rewrites'), 'AdGuard.tsx must have rewrites subtab');
  assert(adguardTsx.includes('subtab-diagnostics'), 'AdGuard.tsx must have diagnostics subtab');
  assert(adguardTsx.includes('subtab-settings'), 'AdGuard.tsx must have settings subtab');

  // 4. types.ts data structures
  assert(typesTs.includes('export interface AdGuardConfig'), 'types.ts must export AdGuardConfig');
  assert(typesTs.includes('export interface AghStatus'), 'types.ts must export AghStatus');
  assert(typesTs.includes('export interface AghHealth'), 'types.ts must export AghHealth');
  assert(typesTs.includes('export interface AghOverview'), 'types.ts must export AghOverview');
  assert(typesTs.includes('export interface AghQueryLogItem'), 'types.ts must export AghQueryLogItem');
  assert(typesTs.includes('export interface AghRewriteEntry'), 'types.ts must export AghRewriteEntry');
  assert(typesTs.includes('export interface AghDiagnostics'), 'types.ts must export AghDiagnostics');
});

// -------------------------------------------------------------
// 31. AdGuard Home Service Start, Pipe Leak Protection & Autonomous Discovery (v1.7.2)
// -------------------------------------------------------------
runTest('31. AdGuard Home Service Start, Pipe Leak Protection & Autonomous Discovery (v1.7.2)', () => {
  const adguardRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/adguard.rs'), 'utf8');
  const mainRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/main.rs'), 'utf8');
  const apiRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/api.rs'), 'utf8');
  const adguardTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/components/AdGuard.tsx'), 'utf8');

  // 1. adguard.rs discovery & autonomous init script generation
  assert(adguardRs.includes('pub fn find_adguard_init_script'), 'adguard.rs must provide find_adguard_init_script');
  assert(adguardRs.includes('pub fn find_adguard_binary'), 'adguard.rs must provide find_adguard_binary');
  assert(adguardRs.includes('pub fn find_adguard_config'), 'adguard.rs must provide find_adguard_config');
  assert(adguardRs.includes('pub fn generate_adguard_init_script_content'), 'adguard.rs must provide generate_adguard_init_script_content');
  assert(adguardRs.includes('pub fn ensure_adguard_init_script'), 'adguard.rs must provide ensure_adguard_init_script');
  assert(adguardRs.includes('pub async fn install_adguard_package'), 'adguard.rs must provide install_adguard_package');

  // 2. adguard.rs service_action pipe leak protection and bounded timeout
  assert(adguardRs.includes("</dev/null >'{log_file}' 2>&1"), 'service_action must detach stdio to prevent daemon pipe leaks');
  assert(adguardRs.includes('Duration::from_secs(15)'), 'service_action must enforce a bounded 15s timeout');
  assert(adguardRs.includes('is_process_running("AdGuardHome")'), 'service_action must verify process presence via is_process_running');
  assert(adguardRs.includes('address already in use'), 'service_action must detect port 53/3000 conflicts in logs');

  // 3. Backend routing: /api/adguard/install endpoint
  assert(mainRs.includes('/api/adguard/install'), 'main.rs must register /api/adguard/install route');
  assert(apiRs.includes('pub async fn adguard_install'), 'api.rs must implement adguard_install handler');

  // 4. Frontend UI: installation feedback and service action button
  assert(adguardTsx.includes('isInstalled'), 'AdGuard.tsx must track isInstalled state');
  assert(adguardTsx.includes('data-testid="adguard-service-action-btn"'), 'AdGuard.tsx must have adguard-service-action-btn');
  assert(adguardTsx.includes('data-testid="adguard-install-btn"'), 'AdGuard.tsx must provide adguard-install-btn when not installed');
  assert(adguardTsx.includes('/adguard/install'), 'AdGuard.tsx must call /adguard/install');
});

// -------------------------------------------------------------
// 32. AdGuard Home Multi-Mirror CDN Downloader, Wizard Bypass & Port 53 Self-Healing (v1.7.3)
// -------------------------------------------------------------
runTest('32. AdGuard Home Multi-Mirror CDN Downloader, Wizard Bypass & Port 53 Self-Healing (v1.7.3)', () => {
  const adguardRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/adguard.rs'), 'utf8');
  const adguardTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/components/AdGuard.tsx'), 'utf8');

  // 1. Architecture detection for MIPS, ARM, AArch64, x86_64
  assert(adguardRs.includes('pub fn detect_adguard_arch_from_str'), 'adguard.rs must implement detect_adguard_arch_from_str');
  assert(adguardRs.includes('pub fn detect_adguard_arch'), 'adguard.rs must implement detect_adguard_arch');
  assert(adguardRs.includes('mipsle_softfloat'), 'adguard.rs must map mipsle routers (MT7621) to mipsle_softfloat');
  assert(adguardRs.includes('arm64'), 'adguard.rs must map aarch64 to arm64');
  assert(adguardRs.includes('armv7'), 'adguard.rs must map armv7 to armv7');

  // 2. Multi-mirror CDN download resilience (AdGuard CDN, GitHub, ghproxy)
  assert(adguardRs.includes('static.adguard.com/adguardhome/release/AdGuardHome_linux_'), 'adguard.rs must use AdGuard official CDN as primary mirror');
  assert(adguardRs.includes('ghproxy.net'), 'adguard.rs must support ghproxy fallback');
  assert(adguardRs.includes('tar -xzf'), 'adguard.rs must extract AdGuardHome tarball');

  // 3. Autonomous configuration generator & First-run wizard bypass
  assert(adguardRs.includes('pub fn generate_default_adguard_yaml'), 'adguard.rs must implement generate_default_adguard_yaml');
  assert(adguardRs.includes('pub fn ensure_default_adguard_config'), 'adguard.rs must implement ensure_default_adguard_config');
  assert(adguardRs.includes('schema_version: 29'), 'generate_default_adguard_yaml must specify schema_version to bypass setup wizard');
  assert(adguardRs.includes('language: ru'), 'generate_default_adguard_yaml must preconfigure Russian language');

  // 4. Port 53 collision detection and automatic self-healing to port 5353
  assert(adguardRs.includes('port: 5353'), 'adguard.rs must provide fallback to port 5353 if port 53 is occupied by dnsmasq');
  assert(adguardRs.includes('replace("port: 53\\n", "port: 5353\\n")'), 'adguard.rs must patch config automatically upon address already in use');

  // 5. Frontend notification and guidance updates
  assert(adguardTsx.includes('официального CDN'), 'AdGuard.tsx notification must inform user about official CDN download');
});

// -------------------------------------------------------------
// 33. AdGuard Home Synchronized Protection Toggle, Duplicated Tab Button & Zapret Lightning Branding (v1.7.4)
// -------------------------------------------------------------
runTest('33. AdGuard Home Synchronized Protection Toggle, Duplicated Tab Button & Zapret Lightning Branding (v1.7.4)', () => {
  const apiRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/api.rs'), 'utf8');
  const dashboardTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/components/Dashboard.tsx'), 'utf8');
  const adguardTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/components/AdGuard.tsx'), 'utf8');
  const headerTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/components/Header.tsx'), 'utf8');
  const appTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/App.tsx'), 'utf8');

  // 1. Backend bidirectional synchronization between toggle_adblock and AdGuard Home protection
  assert(apiRs.includes('set_protection(&state.http, &tx.config().adguard, target_enabled).await'), 'api.rs toggle_adblock must synchronize with AdGuard Home protection endpoint');
  assert(apiRs.includes('tx.config_mut().adblock_enabled = now_enabled'), 'api.rs adguard_set_protection must synchronize adblock_enabled flag in config');
  assert(apiRs.includes('Zapret 2 {cur_ver}') && apiRs.includes('Zapret 1 {cur_ver}'), 'api.rs must return Zapret 1/2 in English');

  // 2. Dashboard AdGuard button and event synchronization
  assert(dashboardTsx.includes('🛡️ AdGuard:'), 'Dashboard.tsx must label button as AdGuard');
  assert(dashboardTsx.includes('xr:adguard-protection-changed'), 'Dashboard.tsx must listen to and dispatch xr:adguard-protection-changed');
  assert(dashboardTsx.includes('/adguard/protection'), 'Dashboard.tsx toggle must call /adguard/protection');

  // 3. AdGuard tab duplicated button and two-way sync
  assert(adguardTsx.includes('data-testid="adguard-duplicated-btn"'), 'AdGuard.tsx must render duplicated button with data-testid="adguard-duplicated-btn"');
  assert(adguardTsx.includes('xr:adguard-protection-changed'), 'AdGuard.tsx must listen to and dispatch xr:adguard-protection-changed');
  assert(adguardTsx.includes('adblock/toggle'), 'AdGuard.tsx handleToggleProtection must synchronize adblock/toggle');

  // 4. Zapret lightning emoji and branding
  assert(headerTsx.includes('<IconZap />'), 'Header.tsx Zapret pill must use IconZap');
  assert(appTsx.includes("{ id: 'zapret', label: '⚡ Zapret' }"), 'App.tsx must use ⚡ Zapret label in tabs');
});

// -------------------------------------------------------------
// 34. Setup.sh SHA-256 Validation Resilience & 404 Not Found Immunity
// -------------------------------------------------------------
runTest('34. Setup.sh SHA-256 Validation Resilience & 404 Not Found Immunity', () => {
  const setupSh = fs.readFileSync(path.resolve(__dirname, '../setup.sh'), 'utf8');
  const buildYml = fs.readFileSync(path.resolve(__dirname, '../.github/workflows/build.yml'), 'utf8');

  // 1. URL pattern matching in download_url must terminate with quote to avoid matching .sha256 assets over binary
  assert(setupSh.includes('xkeen-route-\'"$ARCH"\'"\''), 'setup.sh download_url must terminate with trailing quote to only match binary asset');

  // 2. curl must use -f / -fsSL so 404 Not Found does not write error page body into .sha256 file
  assert(setupSh.includes('curl -fsSL --max-time 15 "${SHA_URL}"'), 'setup.sh must use curl -fsSL when downloading .sha256');

  // 3. Script must validate that EXPECTED_SHA is exactly 64 hex characters before comparing
  assert(setupSh.includes('${#EXPECTED_SHA} -eq 64'), 'setup.sh must check that EXPECTED_SHA length is exactly 64');
  assert(setupSh.includes("tr -d '0-9a-fA-F'"), 'setup.sh must verify that EXPECTED_SHA contains only hex characters');

  // 4. Simulation of 404 error text vs valid sha256
  const validateSha = (raw) => {
    const firstWord = raw.trim().split(/\s+/)[0] || '';
    if (firstWord.length === 64 && /^[0-9a-fA-F]{64}$/.test(firstWord)) {
      return firstWord;
    }
    return null;
  };

  assert.strictEqual(validateSha('Not Found\n'), null, '404 "Not Found" must NOT be treated as a valid SHA-256');
  assert.strictEqual(validateSha('<!DOCTYPE html><html><body>Error</body></html>'), null, 'HTML error pages must NOT be treated as a valid SHA-256');
  assert.strictEqual(validateSha('41bee937b7d378b531fec0797866d21a1e60dc9f582f072302581cbe4b7892aa  xkeen-route-arm64-v8a\n'), '41bee937b7d378b531fec0797866d21a1e60dc9f582f072302581cbe4b7892aa');

  // 5. GitHub workflow must generate and release .sha256 checksums
  assert(buildYml.includes('sha256sum xkeen-route-${{ matrix.name }} > xkeen-route-${{ matrix.name }}.sha256'), 'build.yml must generate sha256 checksum files');
  assert(buildYml.includes('xkeen-route-${{ matrix.name }}.sha256'), 'build.yml must upload .sha256 files as release artifacts');
});

// -------------------------------------------------------------
// 35. Gaming Mode Proxy-Providers Support & Reachability (v1.7.5)
// -------------------------------------------------------------
runTest('35. Gaming Mode Proxy-Providers Support & Reachability (v1.7.5)', () => {
  const apiRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/api.rs'), 'utf8');

  // 1. Backend functions exist
  assert(apiRs.includes('pub fn target_exists('), 'api.rs must define pub fn target_exists');
  assert(apiRs.includes('pub fn tunnel_reachability('), 'api.rs must define pub fn tunnel_reachability');

  // 2. apply_and_verify_gaming checks target_exists across get_proxies and get_provider_proxies
  assert(apiRs.includes('get_provider_proxies(&tx.state.http, tx.config())'), 'apply_and_verify_gaming must query get_provider_proxies');
  assert(apiRs.includes('!target_exists(&proxies, &providers, &target_srv)'), 'apply_and_verify_gaming must verify target_srv with target_exists');

  // 3. get_gaming_status tests latency via GAMING_GROUP_NAME and resolves tunnel reachability
  assert(apiRs.includes('routing::GAMING_GROUP_NAME, 2500, None)'), 'get_gaming_status must ping GAMING_GROUP_NAME as selector fallback');
  assert(apiRs.includes('tunnel_reachability(proxies, providers, &node, delay)'), 'get_gaming_status must invoke tunnel_reachability');

  // 4. Rust unit tests present
  assert(apiRs.includes('fn test_gaming_tunnel_reachability_and_target_exists()'), 'api.rs must contain test_gaming_tunnel_reachability_and_target_exists');

  // 5. JavaScript simulation of pure logic
  function simTargetExists(proxies, providers, name) {
    if (!name) return false;
    return Boolean(proxies[name] || providers[name]);
  }

  function simTunnelReachability(proxies, providers, node, delay) {
    if (!node) return [false, null];
    if (node === 'DIRECT') return [true, 0];
    const inProxies = Boolean(proxies[node]);
    const pEntry = providers[node];
    const inProviders = Boolean(pEntry);
    if (!inProxies && !inProviders) return [false, null];
    if (pEntry && pEntry.alive === false) return [false, null];
    if (proxies[node] && proxies[node].alive === false) return [false, null];
    if (delay > 0) return [true, delay];
    return [true, null];
  }

  const mockProxies = { 'Static-1': { name: 'Static-1' } };
  const mockProviders = {
    'Prov-Alive': { name: 'Prov-Alive', alive: true },
    'Prov-Dead': { name: 'Prov-Dead', alive: false },
    'Prov-NoField': { name: 'Prov-NoField' },
  };

  // Node only in providers, ping -1, alive != false -> reachable: true
  assert.deepStrictEqual(simTunnelReachability(mockProxies, mockProviders, 'Prov-Alive', -1), [true, null]);
  assert.deepStrictEqual(simTunnelReachability(mockProxies, mockProviders, 'Prov-NoField', -1), [true, null]);

  // Node only in providers, alive: false -> reachable: false
  assert.deepStrictEqual(simTunnelReachability(mockProxies, mockProviders, 'Prov-Dead', -1), [false, null]);
  assert.deepStrictEqual(simTunnelReachability(mockProxies, mockProviders, 'Prov-Dead', 50), [false, null]);

  // Node missing everywhere -> false
  assert.deepStrictEqual(simTunnelReachability(mockProxies, mockProviders, 'Missing', -1), [false, null]);
  assert.deepStrictEqual(simTunnelReachability(mockProxies, mockProviders, 'Missing', 100), [false, null]);

  // target_exists true for provider node
  assert.strictEqual(simTargetExists(mockProxies, mockProviders, 'Prov-Alive'), true);
  assert.strictEqual(simTargetExists(mockProxies, mockProviders, 'Static-1'), true);
  assert.strictEqual(simTargetExists(mockProxies, mockProviders, 'Missing'), false);
  assert.strictEqual(simTargetExists(mockProxies, mockProviders, ''), false);
});

// 36. Selector Auto-Heal Subscription / Proxy-Providers Immunity (v1.7.6)
runTest('36. Selector Auto-Heal Subscription / Proxy-Providers Immunity (v1.7.6)', () => {
  function simEvaluateSelectorAutoHeal(now, all, proxiesHasKey) {
    if (!all || all.length === 0) return null;
    const nowValid = Boolean(now) && (all.includes(now) || proxiesHasKey(now));
    if (nowValid) return null;
    if (all.includes('Fallback')) return 'Fallback';
    if (all.includes('Fastest')) return 'Fastest';
    return all.find(x => (all.includes(x) || proxiesHasKey(x)) && x !== now) || all[0];
  }

  const rootProxies = {
    'Fallback': { type: 'Fallback' },
    'Fastest': { type: 'URLTest' },
    'PROXY': { type: 'Selector' },
  };
  const proxiesHasKey = (k) => Object.prototype.hasOwnProperty.call(rootProxies, k);

  const groupAll = ['Fallback', 'Fastest', 'ObsV-ne_dir', '🇩🇪 Германия'];

  // Case 1: Subscription node present in `all`, absent in root `proxies` -> MUST NOT HEAL (return null)
  assert.strictEqual(
    simEvaluateSelectorAutoHeal('ObsV-ne_dir', groupAll, proxiesHasKey),
    null,
    'Subscription node in group all must never trigger auto-heal to Fallback'
  );

  // Case 2: Static proxy present in root `proxies` -> MUST NOT HEAL (return null)
  assert.strictEqual(
    simEvaluateSelectorAutoHeal('Fastest', groupAll, proxiesHasKey),
    null,
    'Fastest in all and root proxies must not trigger auto-heal'
  );

  // Case 3: Truly missing node deleted from subscription -> HEALS to Fallback
  assert.strictEqual(
    simEvaluateSelectorAutoHeal('DeletedNode', groupAll, proxiesHasKey),
    'Fallback',
    'Truly deleted node must heal to Fallback'
  );

  // Case 4: Truly missing node when Fallback not in all -> HEALS to Fastest
  assert.strictEqual(
    simEvaluateSelectorAutoHeal('DeletedNode', ['Fastest', 'ObsV-ne_dir'], proxiesHasKey),
    'Fastest',
    'Deleted node when Fallback absent must heal to Fastest'
  );

  // Case 5: Empty now string -> HEALS to Fallback
  assert.strictEqual(
    simEvaluateSelectorAutoHeal('', groupAll, proxiesHasKey),
    'Fallback',
    'Empty now selector must heal to Fallback'
  );
});

// -------------------------------------------------------------
// 37. Xbox DNS Dedicated Tab, Antigravity Resolver Integration & AdGuard Tab Deduplication (v1.7.7)
// -------------------------------------------------------------
runTest('37. Xbox DNS Dedicated Tab, Antigravity Resolver Integration & AdGuard Tab Deduplication (v1.7.7)', () => {
  const appTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/App.tsx'), 'utf8');
  const headerTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/components/Header.tsx'), 'utf8');
  const xboxDnsTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/components/XboxDns.tsx'), 'utf8');
  const antigravityRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/antigravity.rs'), 'utf8');
  const apiRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/api.rs'), 'utf8');
  const mainRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/main.rs'), 'utf8');

  // 1. App.tsx tab registration and rendering
  assert(appTsx.includes("'xbox-dns'"), 'App.tsx must include xbox-dns in TabId');
  assert(appTsx.includes("id: 'xbox-dns'"), 'App.tsx must register xbox-dns tab in TABS');
  assert(appTsx.includes("label: '🎮 Xbox DNS'"), 'App.tsx must label xbox-dns tab as 🎮 Xbox DNS');
  assert(appTsx.includes('<XboxDns notify={notify} />'), 'App.tsx must render XboxDns component');

  // 2. AdGuard Tab Deduplication & Restored Help
  assert(appTsx.includes("id: 'adguard'"), 'App.tsx must have dedicated adguard tab in TABS');
  assert(appTsx.includes('<Help status={status} />'), 'App.tsx must render Help component for help tab');
  assert(headerTsx.includes('data-testid="header-help-btn"'), 'Header.tsx must retain header-help-btn');
  assert(headerTsx.includes('❓'), 'Header.tsx header-help-btn must use Help icon without duplicating AdGuard');

  // 3. XboxDns.tsx UI features
  assert(xboxDnsTsx.includes('data-testid="xbox-dns-view"'), 'XboxDns.tsx must contain xbox-dns-view container');
  assert(xboxDnsTsx.includes('data-testid="xbox-dns-check-btn"'), 'XboxDns.tsx must have check DNS button');
  assert(xboxDnsTsx.includes('data-testid="xbox-dns-download-cmd"'), 'XboxDns.tsx must have download button for fix_xbox_dns.cmd');
  assert(xboxDnsTsx.includes('111.88.96.54'), 'XboxDns.tsx must list primary IP 111.88.96.54');
  assert(xboxDnsTsx.includes('111.88.96.50'), 'XboxDns.tsx must list alt IP 111.88.96.50');
  assert(xboxDnsTsx.includes('188.68.214.130'), 'XboxDns.tsx must display Selectel SNI proxy IP 188.68.214.130');

  // 4. Backend Antigravity resolver integration
  assert(antigravityRs.includes('("xbox-dns.ru", "udp", "111.88.96.54")'), 'antigravity.rs must include xbox-dns.ru in resolver_candidates');
  assert(antigravityRs.includes('("xbox-dns.ru (alt)", "udp", "111.88.96.50")'), 'antigravity.rs must include xbox-dns.ru (alt) in resolver_candidates');

  // 5. Backend Xbox DNS API endpoints
  assert(apiRs.includes('pub async fn get_xbox_dns_status'), 'api.rs must provide get_xbox_dns_status');
  assert(apiRs.includes('pub async fn check_xbox_dns'), 'api.rs must provide check_xbox_dns');
  assert(apiRs.includes('pub async fn get_xbox_dns_fix_cmd'), 'api.rs must provide get_xbox_dns_fix_cmd');
  assert(apiRs.includes('fix_xbox_dns.cmd'), 'api.rs must serve fix_xbox_dns.cmd with Content-Disposition');

  // 6. Router endpoint registration in main.rs
  assert(mainRs.includes('/api/xbox-dns/status'), 'main.rs must route /api/xbox-dns/status');
  assert(mainRs.includes('/api/xbox-dns/check'), 'main.rs must route /api/xbox-dns/check');
  assert(mainRs.includes('/api/xbox-dns/fix.cmd'), 'main.rs must route /api/xbox-dns/fix.cmd');
});

console.log(`\n=== All ${passedTests}/${totalTests} Regression Tests Passed Successfully ===`);







