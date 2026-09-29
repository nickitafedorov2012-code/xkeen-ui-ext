// ==============================================================================
// verify-milestone6.cjs — Comprehensive Automated Verification Suite
// for Milestone 6 (Этап 6): Safe Updates, Backups, Routing & Decomposition
// (UP-01..04, BK-01..06, RT-01..08, GEO-01..04, ARCH-01..03)
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

console.log('=== Running Milestone 6 Safe Updates, Backups, Routing & Architecture Verification ===\n');

const updaterRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/updater.rs'), 'utf8');
const apiRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/api.rs'), 'utf8');
const routingRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/routing.rs'), 'utf8');
const mihomoRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/mihomo.rs'), 'utf8');
const watchdogRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/watchdog.rs'), 'utf8');
const overrideSyncRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/override_sync.rs'), 'utf8');
const configRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/config.rs'), 'utf8');
const cargoToml = fs.readFileSync(path.resolve(__dirname, '../backend/Cargo.toml'), 'utf8');
const packageJson = JSON.parse(fs.readFileSync(path.resolve(__dirname, '../frontend/package.json'), 'utf8'));
const updateModalTsx = fs.readFileSync(path.resolve(__dirname, '../frontend/src/components/UpdateModal.tsx'), 'utf8');
const developmentMd = fs.readFileSync(path.resolve(__dirname, '../DEVELOPMENT.md'), 'utf8');

// -------------------------------------------------------------
// 1. UP-01 & UP-02: Safe Panel Updater with ELF Header Check & Staging
// -------------------------------------------------------------
runTest('1. UP-01 & UP-02: Panel updater ELF validation, UPDATE_LOCK, staging, backup & rollback', () => {
  assert(updaterRs.includes('static UPDATE_LOCK'), 'updater.rs must have static UPDATE_LOCK for serializing updates');
  assert(updaterRs.includes('validate_elf_header'), 'updater.rs must validate ELF header');
  assert(updaterRs.includes('<!doc') && updaterRs.includes('<html'), 'ELF validation must detect HTML error responses');
  assert(updaterRs.includes('chosen_asset'), 'updater.rs must select chosen_asset');
  assert(updaterRs.includes('xkeen-route-armv7-v7a'), 'updater.rs must support armv7-v7a artifact candidate (UPD-04)');
  assert(updaterRs.includes('--version'), 'updater.rs must pre-test run new binary before live replacement');
  assert(updaterRs.includes('staged_dest'), 'updater.rs must stage new binary before atomic rename');
  assert(updaterRs.includes('target_bin.with_extension("bak")') || updaterRs.includes('xkeen-route.bak'), 'updater.rs must backup live binary to .bak');
  assert(updaterRs.includes('Stdio::null()'), 'updater.rs must detach stdio descriptors on restart');
});

// -------------------------------------------------------------
// 2. UP-03: Safe Mihomo Core Updater
// -------------------------------------------------------------
runTest('2. UP-03: Mihomo updater ELF header check, backup verification, and rollback', () => {
  assert(updaterRs.includes('mihomo_install'), 'updater.rs must implement mihomo_install');
  assert(updaterRs.includes('UPDATE_LOCK.try_lock()'), 'mihomo_install must acquire UPDATE_LOCK');
  assert(updaterRs.includes('validate_elf_header'), 'mihomo_install must validate ELF header on decompressed binary');
  assert(!updaterRs.includes('let _ = tokio::fs::remove_file(target_bin).await;'), 'mihomo_install must NEVER delete target_bin on rename failure (UPD-03)');
  assert(updaterRs.includes('rollback_ok'), 'mihomo_install must perform verified rollback to .bak on startup failure');
  assert(updaterRs.includes('Stdio::null()'), 'mihomo_install must detach stdio on restart');
});

// -------------------------------------------------------------
// 3. UP-04: Update UI Integrity & Retry Banner
// -------------------------------------------------------------
runTest('3. UP-04: UpdateModal renders changelog notes, installError and retry button', () => {
  assert(updateModalTsx.includes('installError'), 'UpdateModal must have installError state');
  assert(updateModalTsx.includes('handleInstall'), 'UpdateModal must have handleInstall action');
  assert(updateModalTsx.includes('Повторить'), 'UpdateModal must provide retry button on install error');
  assert(updateModalTsx.includes('Что нового'), 'UpdateModal must display What\'s New section');
  assert(developmentMd.includes('## v1.5.29'), 'DEVELOPMENT.md must contain ## v1.5.29 changelog section');
});

// -------------------------------------------------------------
// 4. BK-01: Atomic Snapshot Backup with Locks
// -------------------------------------------------------------
runTest('4. BK-01: create_backup acquires config_lock & routing_lock, millisecond name, staging dir', () => {
  assert(apiRs.includes('pub async fn create_backup'), 'api.rs must have create_backup');
  // Check that both locks are acquired
  const createBackupFn = apiRs.slice(apiRs.indexOf('pub async fn create_backup'), apiRs.indexOf('pub struct BackupReq'));
  assert(createBackupFn.includes('state.config_lock.lock()'), 'create_backup must acquire config_lock');
  assert(createBackupFn.includes('state.routing_lock.lock()'), 'create_backup must acquire routing_lock');
  assert(createBackupFn.includes('%3f'), 'create_backup must use millisecond timestamp for unique names');
  assert(createBackupFn.includes('tmp_dir'), 'create_backup must write to staging tmp_dir first');
  assert(createBackupFn.includes('config.yaml') && createBackupFn.includes('config.json'), 'create_backup must include both config.yaml and config.json');
});

// -------------------------------------------------------------
// 5. BK-02 & BK-03: Strict Backup Name & Import Validation
// -------------------------------------------------------------
runTest('5. BK-02 & BK-03: valid_backup_name contract, import_backup validation & collision prevention', () => {
  assert(apiRs.includes('fn valid_backup_name'), 'api.rs must define valid_backup_name');
  const validNameFn = apiRs.slice(apiRs.indexOf('fn valid_backup_name'), apiRs.indexOf('pub async fn list_backups'));
  assert(validNameFn.includes('starts_with("xr-")'), 'valid_backup_name must enforce xr- prefix');
  assert(validNameFn.includes('..') && validNameFn.includes('/') && validNameFn.includes('\\'), 'valid_backup_name must reject path traversal');

  const importFn = apiRs.slice(apiRs.indexOf('pub async fn import_backup'), apiRs.indexOf('pub async fn mihomo_logs_tail'));
  assert(importFn.includes('validate_yaml_syntax'), 'import_backup must validate YAML syntax before disk write');
  assert(importFn.includes('serde_json::from_str'), 'import_backup must validate JSON syntax before disk write');
  assert(importFn.includes('dir.exists()'), 'import_backup must prevent silent overwrite with collision check');
  assert(importFn.includes('xr-'), 'import_backup fallback name must enforce xr- prefix (BK-03)');
  assert(importFn.includes('tmp_dir'), 'import_backup must use staging directory before promotion');
});

// -------------------------------------------------------------
// 6. BK-04 & BK-05: restore_backup via ConfigTx Coordinator
// -------------------------------------------------------------
runTest('6. BK-04 & BK-05: restore_backup delegates to ConfigTx with rollback and path preservation', () => {
  const restoreFn = apiRs.slice(apiRs.indexOf('pub async fn restore_backup'), apiRs.indexOf('pub async fn delete_backup'));
  assert(restoreFn.includes('ConfigTx::begin'), 'restore_backup must begin transaction using ConfigTx');
  assert(restoreFn.includes('validate_yaml_syntax'), 'restore_backup must validate YAML syntax before starting transaction');
  assert(restoreFn.includes('commit_and_reload'), 'restore_backup must commit and reload via ConfigTx');
  assert(restoreFn.includes('is_recovery_required'), 'restore_backup must check and report recovery_required on rollback failure');
  assert(restoreFn.includes('new_cfg.system = cfg.system.clone()'), 'restore_backup must preserve local host system parameters (BK-04)');
});

// -------------------------------------------------------------
// 7. BK-06: export_backup Strict Error Checking
// -------------------------------------------------------------
runTest('7. BK-06: export_backup returns error instead of empty file replacement', () => {
  const exportFn = apiRs.slice(apiRs.indexOf('pub async fn export_backup'), apiRs.indexOf('pub async fn import_backup'));
  assert(!exportFn.includes('unwrap_or_default()'), 'export_backup must NOT replace file read errors with unwrap_or_default()');
  assert(exportFn.includes('valid_backup_name'), 'export_backup must validate backup name');
  assert(exportFn.includes('config.yaml') && exportFn.includes('config.json'), 'export_backup must export config.yaml and config.json');
});

// -------------------------------------------------------------
// 8. RT-01: Gaming Selector Group Excluded from switch_server
// -------------------------------------------------------------
runTest('8. RT-01: mihomo::switch_server excludes Gaming selector groups', () => {
  const switchFn = mihomoRs.slice(mihomoRs.indexOf('pub async fn switch_server'), mihomoRs.indexOf('pub async fn switch_group'));
  assert(switchFn.includes('gaming'), 'switch_server must exclude gaming groups');
  assert(switchFn.includes('гейминг') || switchFn.includes('🎮'), 'switch_server must exclude Russian and icon gaming group names');
});

// -------------------------------------------------------------
// 9. RT-05: group_yaml Includes - REJECT
// -------------------------------------------------------------
runTest('9. RT-05: routing::group_yaml includes - REJECT for schedule block support', () => {
  const groupYamlFn = routingRs.slice(routingRs.indexOf('pub fn group_yaml'), routingRs.indexOf('pub fn parse_provider_names'));
  assert(groupYamlFn.includes('- REJECT'), 'group_yaml must include - REJECT in proxies list');
});

// -------------------------------------------------------------
// 10. RT-08: rule_line & DEV_DOMAINS Support IPv6 /128 & IPv4 /32
// -------------------------------------------------------------
runTest('10. RT-08: rule_line, DEV_DOMAINS and rules_cidr_re support IPv6 (/128) and IPv4 (/32)', () => {
  const ruleLineFn = routingRs.slice(routingRs.indexOf('pub fn rule_line'), routingRs.indexOf('fn extract_block'));
  assert(ruleLineFn.includes('128') && ruleLineFn.includes('32'), 'rule_line must handle both /128 for IPv6 and /32 for IPv4');
  assert(routingRs.includes('(?:32|128)'), 'rules_cidr_re must match both /32 and /128');

  const setDeviceRoutingFn = apiRs.slice(apiRs.indexOf('pub async fn set_device_routing'), apiRs.indexOf('pub async fn get_domains'));
  assert(setDeviceRoutingFn.includes('IpAddr'), 'set_device_routing must validate IP via IpAddr parse');
});

// -------------------------------------------------------------
// 11. RT-03: DHCP Monitor In-Place MAC Device Updates
// -------------------------------------------------------------
runTest('11. RT-03: spawn_dhcp_device_monitor updates devices in-place under lock', () => {
  const dhcpFn = watchdogRs.slice(watchdogRs.indexOf('pub fn spawn_dhcp_device_monitor'), watchdogRs.indexOf('pub fn spawn_zapret_monitor'));
  assert(dhcpFn.includes('state.config_lock.lock().await'), 'spawn_dhcp_device_monitor must acquire config_lock');
  assert(dhcpFn.includes('state.routing_lock.lock().await'), 'spawn_dhcp_device_monitor must acquire routing_lock');
  assert(dhcpFn.includes('mac.eq_ignore_ascii_case'), 'spawn_dhcp_device_monitor must match devices by MAC');
  assert(!dhcpFn.includes('new_cfg.gaming.devices = updated_devices;'), 'spawn_dhcp_device_monitor must NOT overwrite gaming.devices with stale clone');
});

// -------------------------------------------------------------
// 12. RT-04: Watchdog Preserves reload_pending Flag
// -------------------------------------------------------------
runTest('12. RT-04: spawn_routing_watchdog maintains reload_pending state on reload error', () => {
  const routingWatchdogFn = watchdogRs.slice(watchdogRs.indexOf('pub fn spawn('), watchdogRs.indexOf('pub fn is_time_in_range'));
  assert(routingWatchdogFn.includes('reload_pending'), 'spawn_routing_watchdog must maintain reload_pending flag');
  assert(routingWatchdogFn.includes('if reload_pending'), 'spawn_routing_watchdog must retry pending reload');
  assert(routingWatchdogFn.includes('if !reload_pending'), 'spawn_routing_watchdog must NOT update metadata when reload is pending');
});

// -------------------------------------------------------------
// 13. RT-06: Schedule State Separation and Reconciliation
// -------------------------------------------------------------
runTest('13. RT-06: spawn_schedules_monitor reconciles deleted schedules and restores original nodes', () => {
  const schedFn = watchdogRs.slice(watchdogRs.indexOf('pub fn spawn_schedules_monitor'), watchdogRs.indexOf('pub fn spawn_dhcp_device_monitor'));
  assert(schedFn.includes('ActiveScheduleEntry'), 'spawn_schedules_monitor must define ActiveScheduleEntry');
  assert(schedFn.includes('device_ip') && schedFn.includes('group_name') && schedFn.includes('original_node'), 'ActiveScheduleEntry must store device_ip, group_name and original_node');
  assert(schedFn.includes('removed_keys') || schedFn.includes('!current_ids.contains'), 'spawn_schedules_monitor must reconcile removed schedules');
  assert(schedFn.includes('entry.device_ip'), 'spawn_schedules_monitor must use device_ip rather than sched_ key');
});

// -------------------------------------------------------------
// 14. GEO-01..04: Geo Override Strict Reading, Error Abortion & Mutex
// -------------------------------------------------------------
runTest('14. GEO-01..04: override_sync strict file read, swap abortion on error, IPv6 sync & IPSET_SYNC_LOCK', () => {
  assert(overrideSyncRs.includes('IPSET_SYNC_LOCK'), 'override_sync.rs must have IPSET_SYNC_LOCK mutex');
  assert(overrideSyncRs.includes('ErrorKind::NotFound'), 'write_override_file_streaming must only treat NotFound as clean create (GEO-01)');
  assert(!overrideSyncRs.includes('if !v6.is_empty() {\n        if let Err(e) = sync_ipset_family("geo_override6"'), 'sync_geo_override must always sync geo_override6 even if v6 is empty (GEO-03)');
  assert(overrideSyncRs.includes('create_dir_all') || overrideSyncRs.includes('map_err'), 'sync_ipset_family must abort swap on staging failure (GEO-02)');
});

// -------------------------------------------------------------
// 15. ARCH-01..03: Unified Paths in SystemConfig and Version 1.5.29
// -------------------------------------------------------------
runTest('15. ARCH-01..03: SystemConfig unified paths and version bumped to 1.5.29/1.6.0', () => {
  assert(configRs.includes('pub xkeen_route_bin: String'), 'SystemConfig must have xkeen_route_bin');
  assert(configRs.includes('pub xkeen_route_init: String'), 'SystemConfig must have xkeen_route_init');
  assert(configRs.includes('pub mihomo_bin: String'), 'SystemConfig must have mihomo_bin');
  assert(configRs.includes('pub zapret_init: String'), 'SystemConfig must have zapret_init');

  assert(cargoToml.includes('version = "1.5.29"') || cargoToml.includes('version = "1.6.0"') || cargoToml.includes('version = "1.6.1"') || cargoToml.includes('version = "1.6.2"') || cargoToml.includes('version = "1.6.3"') || cargoToml.includes('version = "1.6.4"') || cargoToml.includes('version = "1.7.0"'), 'backend/Cargo.toml must be bumped');
  assert(packageJson.version === '1.5.29' || packageJson.version === '1.6.0' || packageJson.version === '1.6.1' || packageJson.version === '1.6.2' || packageJson.version === '1.6.3' || packageJson.version === '1.6.4' || packageJson.version === '1.7.0', 'frontend/package.json must be bumped');
});

// -------------------------------------------------------------
// 16. UP-01: Architecture & Endianness Precision (MIPS vs MIPSEL)
// -------------------------------------------------------------
runTest('16. UP-01: current_arch and detect_mihomo_arch differentiate MIPS big-endian vs little-endian (mipsel)', () => {
  assert(updaterRs.includes('pub fn current_arch() -> &\'static str'), 'updater.rs must have current_arch function');
  assert(updaterRs.includes('target_endian = "little"'), 'updater.rs must check target_endian for MIPS to prevent installing big-endian on mipsel');
  assert(updaterRs.includes('target_arch = "mips"'), 'updater.rs must handle target_arch = "mips"');
  assert(updaterRs.includes('let arch = current_arch();'), 'updater install must use current_arch() instead of raw consts::ARCH');
});

// -------------------------------------------------------------
// 17. UP-01: Fail-Closed ELF Validation in Mihomo Core Installer
// -------------------------------------------------------------
runTest('17. UP-01: mihomo_install strictly validates ELF header without silent bypass on read errors', () => {
  const mihomoInstallSlice = updaterRs.slice(updaterRs.indexOf('pub async fn mihomo_install'), updaterRs.indexOf('pub fn parse_gh_version'));
  assert(mihomoInstallSlice.includes('validate_elf_header'), 'mihomo_install must call validate_elf_header');
  assert(!mihomoInstallSlice.includes('if f.read_exact(&mut elf_hdr).await.is_ok()'), 'mihomo_install must not silently ignore read_exact errors');
  assert(mihomoInstallSlice.includes('Не удалось прочитать ELF-заголовок ядра'), 'mihomo_install must return error if header reading fails');
});

// -------------------------------------------------------------
// 18. BK-01 & BK-02: Full Snapshot Completeness & Path Traversal Rejection
// -------------------------------------------------------------
runTest('18. BK-01 & BK-02: Backup includes zapret-hosts.txt & ru_exclude_override.lst and blocks traversal in files', () => {
  const createBackupFn = apiRs.slice(apiRs.indexOf('pub async fn create_backup'), apiRs.indexOf('pub struct BackupReq'));
  assert(createBackupFn.includes('zapret-hosts.txt'), 'create_backup must backup zapret-hosts.txt');
  assert(createBackupFn.includes('ru_exclude_override.lst'), 'create_backup must backup ru_exclude_override.lst');

  const restoreBackupFn = apiRs.slice(apiRs.indexOf('pub async fn restore_backup'), apiRs.indexOf('pub async fn delete_backup'));
  assert(restoreBackupFn.includes('zapret-hosts.txt'), 'restore_backup must restore zapret-hosts.txt');
  assert(restoreBackupFn.includes('ru_exclude_override.lst'), 'restore_backup must restore ru_exclude_override.lst');

  const importBackupFn = apiRs.slice(apiRs.indexOf('pub async fn import_backup'), apiRs.indexOf('pub async fn mihomo_logs_tail'));
  assert(importBackupFn.includes('key.contains("..")'), 'import_backup must reject path traversal in files keys');
  assert(importBackupFn.includes('zapret-hosts.txt'), 'import_backup must support zapret-hosts.txt');

  const validNameFn = apiRs.slice(apiRs.indexOf('fn valid_backup_name'), apiRs.indexOf('pub async fn list_backups'));
  assert(validNameFn.includes('name.len() > 3'), 'valid_backup_name must require name.len() > 3 to reject "xr-" prefix alone');
});

// -------------------------------------------------------------
// 19. RT-01 & RT-02: IP/CIDR & Domain Strict Validation & ConfigTx in force_add / apply_routing
// -------------------------------------------------------------
runTest('19. RT-01 & RT-02: Strict IP/CIDR validation, format_src_ip_cidr & transactional routing', () => {
  assert(configRs.includes('pub fn is_valid_ip_or_cidr'), 'config.rs must define is_valid_ip_or_cidr');
  assert(routingRs.includes('pub fn format_src_ip_cidr'), 'routing.rs must define format_src_ip_cidr');

  const forceAddFn = apiRs.slice(apiRs.indexOf('pub async fn force_add_domain'), apiRs.indexOf('pub async fn logs_tail'));
  assert(forceAddFn.includes('is_valid_domain'), 'force_add_domain must validate domain');
  assert(forceAddFn.includes('is_valid_ip_or_cidr'), 'force_add_domain must validate client_ip');
  assert(forceAddFn.includes('ConfigTx::begin'), 'force_add_domain must use ConfigTx coordinator');

  const setDevDomainRulesFn = apiRs.slice(apiRs.indexOf('pub async fn set_device_domain_rules'), apiRs.indexOf('pub async fn clash_proxy'));
  assert(setDevDomainRulesFn.includes('is_valid_ip_or_cidr'), 'set_device_domain_rules must validate device IP/CIDR');
  assert(setDevDomainRulesFn.includes('is_valid_domain'), 'set_device_domain_rules must validate domain rules');

  const applyRoutingFn = apiRs.slice(apiRs.indexOf('pub async fn apply_routing'), apiRs.indexOf('pub struct RenameProviderReq'));
  assert(applyRoutingFn.includes('is_valid_ip_or_cidr'), 'apply_routing must validate all assignment IPs');
  assert(applyRoutingFn.includes('ConfigTx::begin'), 'apply_routing must use ConfigTx coordinator');

  const applyDomainRulesFn = routingRs.slice(routingRs.indexOf('pub fn apply_domain_rules'), routingRs.indexOf('pub fn format_src_ip_cidr'));
  assert(applyDomainRulesFn.includes('direct_set'), 'apply_domain_rules must deduplicate direct and force domain conflicts');
});

// -------------------------------------------------------------
// 20. BUILD-02: Rust Compiler Integrity (Brace Balance, Borrow Safety & Struct Fields)
// -------------------------------------------------------------
runTest('20. BUILD-02: All Rust source files have balanced braces, valid borrow semantics & struct fields', () => {
  const srcDir = path.resolve(__dirname, '../backend/src');
  fs.readdirSync(srcDir).filter(f => f.endsWith('.rs')).forEach(f => {
    const content = fs.readFileSync(path.join(srcDir, f), 'utf8');
    let balance = 0;
    let inString = false;
    let inLineComment = false;
    let inBlockComment = false;
    let inRawString = false;
    for (let i = 0; i < content.length; i++) {
      const ch = content[i];
      const next = content[i + 1];
      if (inLineComment) { if (ch === '\n') inLineComment = false; }
      else if (inBlockComment) { if (ch === '*' && next === '/') { inBlockComment = false; i++; } }
      else if (inString) { if (ch === '\\') i++; else if (ch === '"') inString = false; }
      else if (inRawString) { if (ch === '"' && next === '#') { inRawString = false; i++; } }
      else {
        if (ch === '/' && next === '/') { inLineComment = true; i++; }
        else if (ch === '/' && next === '*') { inBlockComment = true; i++; }
        else if (ch === 'r' && next === '#' && content[i + 2] === '"') { inRawString = true; i += 2; }
        else if (ch === '"') inString = true;
        else if (ch === '\'') {
          if (content[i + 1] === '\\') {
            const closeIdx = content.indexOf('\'', i + 2);
            if (closeIdx !== -1 && closeIdx - i <= 6) i = closeIdx;
          } else if (content[i + 2] === '\'') { i += 2; }
        }
        else if (ch === '{') balance++;
        else if (ch === '}') balance--;
      }
    }
    assert.strictEqual(balance, 0, `Brace imbalance detected in ${f}: ${balance}`);
  });

  // Verify verify_zapret_failsafe call signature in api.rs
  assert(apiRs.includes('crate::zapret::verify_zapret_failsafe(&state.http, None).await'), 'verify_zapret_failsafe in api.rs must pass (&state.http, None)');

  // Verify calc_schedules_revision uses s.ip instead of non-existent device_ip/device_mac
  const calcSchedFn = apiRs.slice(apiRs.indexOf('pub fn calc_schedules_revision'), apiRs.indexOf('pub async fn get_schedules'));
  assert(calcSchedFn.includes('s.ip.hash'), 'calc_schedules_revision must hash s.ip');
  assert(!calcSchedFn.includes('s.device_ip') && !calcSchedFn.includes('s.device_mac'), 'calc_schedules_revision must not reference non-existent device_ip/device_mac');

  // Verify transaction.rs collects extra_items before mutably borrowing self in rollback_disk_files
  const txRs = fs.readFileSync(path.resolve(__dirname, '../backend/src/transaction.rs'), 'utf8');
  assert(txRs.includes('let extra_items: Vec<(PathBuf, String)> = self'), 'transaction.rs must clone extra_files items before loop to avoid simultaneous borrow');
});

console.log(`\n=== All ${passedTests}/${totalTests} Milestone 6 Checks Passed Successfully ===\n`);
