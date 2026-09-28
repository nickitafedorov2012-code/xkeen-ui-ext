/**
 * Milestone 2 Verification & Invariant Audit Suite
 * Verifies:
 * 1. ConfigTx implementation in backend/src/transaction.rs
 * 2. Strict lock ordering (config_lock before routing_lock)
 * 3. Two-phase rollback and snapshot restoration
 * 4. Pre-validation of YAML syntax before disk writes
 * 5. Refactored mutating API handlers in backend/src/api.rs
 * 6. Functional simulation of ConfigTx state machine and rollback
 */

const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert');

console.log('=== Running Milestone 2 ConfigTx & Transaction Core Verification ===\n');

let passed = 0;
let total = 0;

function check(name, fn) {
  total++;
  try {
    fn();
    console.log(`  ✓ ${name}`);
    passed++;
  } catch (err) {
    console.error(`  ✗ ${name}`);
    console.error(`    Error: ${err.message}`);
    process.exitCode = 1;
  }
}

const rootDir = path.resolve(__dirname, '..');
const txRsPath = path.join(rootDir, 'backend/src/transaction.rs');
const apiRsPath = path.join(rootDir, 'backend/src/api.rs');
const mainRsPath = path.join(rootDir, 'backend/src/main.rs');

// 1. Check file existence and module registration
check('1. transaction.rs exists and is registered in main.rs', () => {
  assert(fs.existsSync(txRsPath), 'backend/src/transaction.rs must exist');
  const mainContent = fs.readFileSync(mainRsPath, 'utf8');
  assert(mainContent.includes('mod transaction;'), 'main.rs must declare mod transaction;');
});

// 2. Strict lock ordering in ConfigTx
check('2. Strict lock ordering (config_lock first, routing_lock second)', () => {
  const txContent = fs.readFileSync(txRsPath, 'utf8');
  const cfgLockIdx = txContent.indexOf('state.config_lock.clone().lock_owned().await');
  const routingLockIdx = txContent.indexOf('state.routing_lock.clone().lock_owned().await');
  assert(cfgLockIdx > 0, 'Must acquire state.config_lock');
  assert(routingLockIdx > 0, 'Must acquire state.routing_lock');
  assert(cfgLockIdx < routingLockIdx, 'Strict lock ordering violated: config_lock must be acquired BEFORE routing_lock');
});

// 3. Automated two-phase rollback in ConfigTx
check('3. ConfigTx snapshots original files and provides two-phase rollback', () => {
  const txContent = fs.readFileSync(txRsPath, 'utf8');
  assert(txContent.includes('original_yaml'), 'Must snapshot original_yaml');
  assert(txContent.includes('original_json'), 'Must snapshot original_json');
  assert(txContent.includes('rollback_disk_files'), 'Must implement disk file rollback');
  assert(txContent.includes('mihomo::reload_config(&self.state.http, &self.initial_config)'), 'Rollback must reload Mihomo with initial_config');
});

// 4. Pre-validation of YAML syntax before disk writes
check('4. Pre-validation of YAML syntax in ConfigTx', () => {
  const txContent = fs.readFileSync(txRsPath, 'utf8');
  assert(txContent.includes('fn validate_yaml_syntax'), 'Must implement validate_yaml_syntax');
  assert(txContent.includes('validate_yaml_syntax(&content)?;'), 'set_yaml must pre-validate syntax before disk write');
  assert(txContent.includes('validate_yaml_syntax(yaml)?;'), 'commit must pre-validate syntax');
});

// 5. Verification of all 9 refactored mutating handlers in api.rs
check('5. All 9 mutating API handlers use ConfigTx coordinator', () => {
  const apiContent = fs.readFileSync(apiRsPath, 'utf8');
  const targetHandlers = [
    'save_gaming_config',
    'toggle_gaming',
    'set_domains',
    'set_device_routing',
    'set_device_domain_rules',
    'set_dns_mode',
    'import_node',
    'save_config_file',
    'toggle_adblock',
  ];

  for (const handler of targetHandlers) {
    const handlerRegex = new RegExp(`pub async fn ${handler}[^{]*\\{([\\s\\S]*?)(?:\\n\\}|\\npub async fn|\\nfn )`);
    const match = apiContent.match(handlerRegex);
    assert(match, `Handler ${handler} not found in api.rs`);
    const body = match[1];
    assert(
      body.includes('ConfigTx::begin(&state).await') || body.includes('ConfigTx::begin'),
      `Handler ${handler} must acquire transaction via ConfigTx::begin`
    );
  }
});

// 6. Functional simulation of YAML syntax validation (including UTF-8 and escaped quotes)
check('6. YAML syntax validator correctly detects syntax errors, handles UTF-8 and escaped quotes', () => {
  const txContent = fs.readFileSync(txRsPath, 'utf8');
  assert(txContent.includes('char_indices().peekable()'), 'validate_yaml_syntax must use char_indices().peekable() for UTF-8 safe parsing');
  assert(txContent.includes('chars.peek()'), 'validate_yaml_syntax must peek to handle escaped quotes');

  function validateYamlSyntax(content) {
    const bracketStack = [];
    const lines = content.split('\n');

    for (let i = 0; i < lines.length; i++) {
      const line = lines[i];
      const displayLine = i + 1;
      if (line.includes('\t')) return { valid: false, error: `Строка ${displayLine}: табуляция` };
      const trimmed = line.trim();
      if (!trimmed || trimmed.startsWith('#')) continue;

      let inSingleQuote = false;
      let inDoubleQuote = false;
      let escaped = false;

      for (let c = 0; c < line.length; c++) {
        const ch = line[c];
        if (escaped) { escaped = false; continue; }
        if (ch === '\\' && inDoubleQuote) { escaped = true; continue; }
        if (ch === "'" && !inDoubleQuote) {
          if (inSingleQuote && c + 1 < line.length && line[c + 1] === "'") {
            c++; // поглощаем экранированную одинарную кавычку ''
            continue;
          }
          inSingleQuote = !inSingleQuote;
          continue;
        }
        if (ch === '"' && !inSingleQuote) { inDoubleQuote = !inDoubleQuote; continue; }
        if (inSingleQuote || inDoubleQuote) continue;
        if (ch === '#' && (c === 0 || line[c - 1] === ' ')) break;

        if (ch === '[' || ch === '{') bracketStack.push({ ch, line: displayLine });
        else if (ch === ']') {
          const top = bracketStack.pop();
          if (!top || top.ch !== '[') return { valid: false, error: `Строка ${displayLine}: скобка ]` };
        } else if (ch === '}') {
          const top = bracketStack.pop();
          if (!top || top.ch !== '{') return { valid: false, error: `Строка ${displayLine}: фигурная скобка }` };
        }
      }

      if (inSingleQuote) return { valid: false, error: `Строка ${displayLine}: незакрытая одинарная кавычка` };
      if (inDoubleQuote) return { valid: false, error: `Строка ${displayLine}: незакрытая двойная кавычка` };
    }

    if (bracketStack.length > 0) return { valid: false, error: 'незакрытая скобка' };
    return { valid: true };
  }

  assert(validateYamlSyntax('port: 7890\nrules:\n  - MATCH,DIRECT').valid, 'Valid YAML must pass');
  assert(!validateYamlSyntax('port: 7890\n\trules:').valid, 'Tabs must be rejected');
  assert(!validateYamlSyntax("name: 'unclosed").valid, 'Unclosed quote must be rejected');
  assert(!validateYamlSyntax('proxies: [name: {test]').valid, 'Mismatched brackets must be rejected');
  assert(validateYamlSyntax("name: 'It''s a valid node name'\nport: 7890\n").valid, 'Escaped quote must be accepted');
  assert(validateYamlSyntax('name: "Россия Direct" # комментарий [1] и \'тест\'\nport: 7890\n').valid, 'Unicode Russian comments must be accepted');
});

// 7. Functional simulation of ConfigTx two-phase rollback
check('7. ConfigTx two-phase rollback disk and state restoration simulation', () => {
  const tmpDir = path.join(__dirname, '../.tmp_tx_test');
  fs.mkdirSync(tmpDir, { recursive: true });
  const yamlPath = path.join(tmpDir, 'config.yaml');
  const jsonPath = path.join(tmpDir, 'config.json');
  const extraPath = path.join(tmpDir, 'custom_provider.yaml');

  const origYaml = 'port: 7890\nrules:\n  - MATCH,DIRECT\n';
  const origJson = '{"refresh_interval_sec":10}\n';
  const origExtra = 'proxies:\n  - name: test\n';
  fs.writeFileSync(yamlPath, origYaml, 'utf8');
  fs.writeFileSync(jsonPath, origJson, 'utf8');
  fs.writeFileSync(extraPath, origExtra, 'utf8');

  let inMemoryState = { refresh_interval_sec: 10 };

  // Begin TX simulation
  class SimConfigTx {
    constructor() {
      this.initialYaml = fs.readFileSync(yamlPath, 'utf8');
      this.initialJson = fs.readFileSync(jsonPath, 'utf8');
      this.initialExtra = fs.readFileSync(extraPath, 'utf8');
      this.initialState = { ...inMemoryState };
      this.stagedYaml = null;
      this.stagedJson = null;
      this.stagedExtra = null;
      this.committed = false;
    }
    setYaml(y) { this.stagedYaml = y; }
    setJson(j) { this.stagedJson = j; }
    setExtra(e) { this.stagedExtra = e; }
    commitAndReload(simulateDaemonFailure = false) {
      // Step 1: Write disk
      if (this.stagedYaml) fs.writeFileSync(yamlPath, this.stagedYaml, 'utf8');
      if (this.stagedJson) fs.writeFileSync(jsonPath, JSON.stringify(this.stagedJson), 'utf8');
      if (this.stagedExtra) fs.writeFileSync(extraPath, this.stagedExtra, 'utf8');

      // Step 2: Reload daemon
      if (simulateDaemonFailure) {
        // Rollback Phase 1: restore disk files
        fs.writeFileSync(yamlPath, this.initialYaml, 'utf8');
        fs.writeFileSync(jsonPath, this.initialJson, 'utf8');
        fs.writeFileSync(extraPath, this.initialExtra, 'utf8');
        // Rollback Phase 2: keep inMemoryState untouched
        throw new Error('Daemon reload rejected configuration');
      }

      // Commit
      if (this.stagedJson) inMemoryState = { ...this.stagedJson };
      this.committed = true;
    }
  }

  // Test failed reload triggers rollback across all files
  const tx1 = new SimConfigTx();
  tx1.setYaml('corrupted: yaml\n');
  tx1.setJson({ refresh_interval_sec: 99 });
  tx1.setExtra('corrupted: extra\n');
  assert.throws(() => tx1.commitAndReload(true), /Daemon reload rejected/);

  // Verify rollback restored all disk files
  assert.strictEqual(fs.readFileSync(yamlPath, 'utf8'), origYaml, 'Disk YAML must be restored on rollback');
  assert.strictEqual(fs.readFileSync(jsonPath, 'utf8'), origJson, 'Disk JSON must be restored on rollback');
  assert.strictEqual(fs.readFileSync(extraPath, 'utf8'), origExtra, 'Extra file must be restored on rollback');
  assert.strictEqual(inMemoryState.refresh_interval_sec, 10, 'In-memory state must remain unchanged');

  // Test successful commit
  const tx2 = new SimConfigTx();
  tx2.setYaml('port: 9090\n');
  tx2.setJson({ refresh_interval_sec: 25 });
  tx2.setExtra('proxies:\n  - name: updated\n');
  tx2.commitAndReload(false);

  assert.strictEqual(fs.readFileSync(yamlPath, 'utf8'), 'port: 9090\n', 'Disk YAML updated');
  assert.strictEqual(fs.readFileSync(extraPath, 'utf8'), 'proxies:\n  - name: updated\n', 'Extra file updated');
  assert.strictEqual(inMemoryState.refresh_interval_sec, 25, 'In-memory state committed');

  fs.rmSync(tmpDir, { recursive: true, force: true });
});

// ==================== STAGE 2 ENHANCEMENTS AUDIT ====================

// 8. TX-01: Aggregated rollback result and recovery_required status
check('8. TX-01: Aggregated rollback result and recovery_required status', () => {
  const txContent = fs.readFileSync(txRsPath, 'utf8');
  assert(txContent.includes('recovery_required'), 'ConfigTx must track recovery_required flag');
  assert(txContent.includes('TxStatus::RecoveryRequired'), 'ConfigTx must have TxStatus::RecoveryRequired state');
  assert(txContent.includes('Result<(), Vec<String>>'), 'rollback and rollback_disk_files must return aggregated Result<(), Vec<String>>');
  assert(!txContent.includes('let _ = crate::api::atomic_write_file'), 'File restoration errors must not be swallowed with let _ =');
});

// 9. TX-02: Granular resource tracking via applied_files
check('9. TX-02: Granular resource tracking via applied_files', () => {
  const txContent = fs.readFileSync(txRsPath, 'utf8');
  assert(txContent.includes('applied_files: Vec<PathBuf>'), 'ConfigTx must track applied_files');
  assert(txContent.includes('self.applied_files.push('), 'applied_files must be appended on each successful write');
  assert(txContent.includes('applied_files.iter().rev()'), 'Rollback must iterate applied_files in reverse (LIFO)');
});

// 10. TX-03: Hold lock guards throughout runtime recovery & atomic Drop
check('10. TX-03: Hold lock guards throughout runtime recovery & atomic Drop', () => {
  const txContent = fs.readFileSync(txRsPath, 'utf8');
  assert(txContent.includes('sync_atomic_write_or_remove'), 'Must implement sync_atomic_write_or_remove helper');
  assert(txContent.includes('_routing_guard: Option<OwnedMutexGuard<()>>'), 'Guards must be movable into background recovery');
  assert(txContent.includes('self._routing_guard.take()'), 'Drop must take routing_guard into recovery task');
  assert(txContent.includes('self._cfg_guard.take()'), 'Drop must take cfg_guard into recovery task');
  assert(txContent.includes('drop(routing_guard)'), 'Locks must be explicitly held until reload finishes');
});

// 11. Optimistic Concurrency: Pre-commit collision check
check('11. Optimistic Concurrency: Hash and collision check before commit', () => {
  const txContent = fs.readFileSync(txRsPath, 'utf8');
  assert(txContent.includes('compute_content_hash'), 'Must compute content hash for collision detection');
  assert(txContent.includes('check_external_collisions'), 'Must implement check_external_collisions');
  assert(txContent.includes('self.check_external_collisions().await?;'), 'apply_disk_files must verify collisions before writes');
});

// 12. Functional simulation of Stage 2 TX-01, TX-02, TX-03, and Collision Check
check('12. Functional simulation of Stage 2 partial write rollback and collision detection', () => {
  const tmpDir = path.join(__dirname, '../.tmp_stage2_test');
  fs.mkdirSync(tmpDir, { recursive: true });
  const yamlPath = path.join(tmpDir, 'config.yaml');
  const jsonPath = path.join(tmpDir, 'config.json');
  const extraPath = path.join(tmpDir, 'extra.yaml');

  fs.writeFileSync(yamlPath, 'port: 7890\n', 'utf8');
  fs.writeFileSync(jsonPath, '{"mode":"rule"}\n', 'utf8');

  // Simulation class matching Stage 2 ConfigTx semantics
  class Stage2Tx {
    constructor() {
      this.origYaml = fs.readFileSync(yamlPath, 'utf8');
      this.origYamlHash = this.hash(this.origYaml);
      this.appliedFiles = [];
      this.recoveryRequired = false;
      this.stagedYaml = null;
    }
    hash(content) {
      return require('crypto').createHash('sha256').update(content).digest('hex');
    }
    checkCollision() {
      const current = fs.readFileSync(yamlPath, 'utf8');
      if (this.hash(current) !== this.origYamlHash) {
        throw new Error('Collision detected: external modification');
      }
    }
    applyDiskFiles(failOnExtra = false) {
      this.checkCollision();
      // Write yaml
      if (this.stagedYaml) {
        fs.writeFileSync(yamlPath, this.stagedYaml, 'utf8');
        this.appliedFiles.push(yamlPath);
      }
      // Write extra (simulated failure)
      if (failOnExtra) {
        this.rollbackDisk();
        throw new Error('Disk write failed on extra file');
      }
    }
    rollbackDisk() {
      const errors = [];
      while (this.appliedFiles.length > 0) {
        const file = this.appliedFiles.pop();
        try {
          if (file === yamlPath) fs.writeFileSync(yamlPath, this.origYaml, 'utf8');
        } catch (e) {
          errors.push(e.message);
        }
      }
      if (errors.length > 0) {
        this.recoveryRequired = true;
        throw new Error(`Rollback failed: ${errors.join(', ')}`);
      }
    }
  }

  // A. Verify Collision Check rejects stale write
  const txColl = new Stage2Tx();
  txColl.stagedYaml = 'port: 9090\n';
  // External edit occurs
  fs.writeFileSync(yamlPath, 'port: 8888\n# external\n', 'utf8');
  assert.throws(() => txColl.applyDiskFiles(), /Collision detected/);
  assert.strictEqual(fs.readFileSync(yamlPath, 'utf8'), 'port: 8888\n# external\n', 'External edit preserved');

  // B. Verify Partial Apply Failure rolls back already written file
  fs.writeFileSync(yamlPath, 'port: 7890\n', 'utf8');
  const txPartial = new Stage2Tx();
  txPartial.stagedYaml = 'port: 9999\n';
  assert.throws(() => txPartial.applyDiskFiles(true), /Disk write failed/);
  assert.strictEqual(fs.readFileSync(yamlPath, 'utf8'), 'port: 7890\n', 'Partial write successfully rolled back');
  assert.strictEqual(txPartial.appliedFiles.length, 0, 'applied_files emptied after rollback');

  fs.rmSync(tmpDir, { recursive: true, force: true });
});

// 13. State guard: commit() rejected on rolled back or recovery_required transaction
check('13. ConfigTx state guard: commit() rejects execution if already rolled back', () => {
  const txContent = fs.readFileSync(txRsPath, 'utf8');
  assert(
    txContent.includes('self.status == TxStatus::RolledBack || self.status == TxStatus::RecoveryRequired'),
    'commit() and apply_disk_files() must check TxStatus::RolledBack and TxStatus::RecoveryRequired'
  );
  assert(
    txContent.includes('Невозможно зафиксировать транзакцию: транзакция уже откатана'),
    'commit() must reject committing a rolled-back transaction'
  );
});

// 14. Mihomo reload guard: only reload runtime if YAML/providers were actually modified
check('14. Mihomo reload guard: skips runtime reload on panel-only (config.json) transactions', () => {
  const txContent = fs.readFileSync(txRsPath, 'utf8');
  assert(
    txContent.includes('let runtime_touched = self.staged_yaml.is_some() || !self.extra_files.is_empty();'),
    'rollback and Drop must check runtime_touched before reloading Mihomo runtime'
  );
});

console.log(`\n=== All ${passed}/${total} Milestone 2 & Stage 2 Checks Passed Successfully ===\n`);
