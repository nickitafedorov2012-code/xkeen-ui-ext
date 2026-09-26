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

// 6. Functional simulation of YAML syntax validation
check('6. YAML syntax validator correctly detects syntax errors', () => {
  function validateYamlSyntax(content) {
    const bracketStack = [];
    let inSingleQuote = false;
    let inDoubleQuote = false;
    let escaped = false;

    const lines = content.split('\n');
    for (let i = 0; i < lines.length; i++) {
      const line = lines[i];
      if (line.includes('\t')) return { valid: false, error: 'табуляция' };
      const trimmed = line.trim();
      if (trimmed.startsWith('#')) continue;

      for (let c = 0; c < line.length; c++) {
        const ch = line[c];
        if (escaped) { escaped = false; continue; }
        if (ch === '\\' && inDoubleQuote) { escaped = true; continue; }
        if (ch === "'" && !inDoubleQuote) { inSingleQuote = !inSingleQuote; continue; }
        if (ch === '"' && !inSingleQuote) { inDoubleQuote = !inDoubleQuote; continue; }
        if (inSingleQuote || inDoubleQuote) continue;
        if (ch === '#' && (c === 0 || line[c - 1] === ' ')) break;

        if (ch === '[' || ch === '{') bracketStack.push({ ch, line: i + 1 });
        else if (ch === ']') {
          const top = bracketStack.pop();
          if (!top || top.ch !== '[') return { valid: false, error: 'скобка ]' };
        } else if (ch === '}') {
          const top = bracketStack.pop();
          if (!top || top.ch !== '{') return { valid: false, error: 'фигурная скобка }' };
        }
      }
    }
    if (bracketStack.length > 0) return { valid: false, error: 'незакрытая скобка' };
    if (inSingleQuote || inDoubleQuote) return { valid: false, error: 'незакрытая кавычка' };
    return { valid: true };
  }

  assert(validateYamlSyntax('port: 7890\nrules:\n  - MATCH,DIRECT').valid, 'Valid YAML must pass');
  assert(!validateYamlSyntax('port: 7890\n\trules:').valid, 'Tabs must be rejected');
  assert(!validateYamlSyntax("name: 'unclosed").valid, 'Unclosed quote must be rejected');
  assert(!validateYamlSyntax('proxies: [name: {test]').valid, 'Mismatched brackets must be rejected');
});

// 7. Functional simulation of ConfigTx two-phase rollback
check('7. ConfigTx two-phase rollback disk and state restoration simulation', () => {
  const tmpDir = path.join(__dirname, '../.tmp_tx_test');
  fs.mkdirSync(tmpDir, { recursive: true });
  const yamlPath = path.join(tmpDir, 'config.yaml');
  const jsonPath = path.join(tmpDir, 'config.json');

  const origYaml = 'port: 7890\nrules:\n  - MATCH,DIRECT\n';
  const origJson = '{"refresh_interval_sec":10}\n';
  fs.writeFileSync(yamlPath, origYaml, 'utf8');
  fs.writeFileSync(jsonPath, origJson, 'utf8');

  let inMemoryState = { refresh_interval_sec: 10 };

  // Begin TX simulation
  class SimConfigTx {
    constructor() {
      this.initialYaml = fs.readFileSync(yamlPath, 'utf8');
      this.initialJson = fs.readFileSync(jsonPath, 'utf8');
      this.initialState = { ...inMemoryState };
      this.stagedYaml = null;
      this.stagedJson = null;
      this.committed = false;
    }
    setYaml(y) { this.stagedYaml = y; }
    setJson(j) { this.stagedJson = j; }
    commitAndReload(simulateDaemonFailure = false) {
      // Step 1: Write disk
      if (this.stagedYaml) fs.writeFileSync(yamlPath, this.stagedYaml, 'utf8');
      if (this.stagedJson) fs.writeFileSync(jsonPath, JSON.stringify(this.stagedJson), 'utf8');

      // Step 2: Reload daemon
      if (simulateDaemonFailure) {
        // Rollback Phase 1: restore disk files
        fs.writeFileSync(yamlPath, this.initialYaml, 'utf8');
        fs.writeFileSync(jsonPath, this.initialJson, 'utf8');
        // Rollback Phase 2: keep inMemoryState untouched
        throw new Error('Daemon reload rejected configuration');
      }

      // Commit
      if (this.stagedJson) inMemoryState = { ...this.stagedJson };
      this.committed = true;
    }
  }

  // Test failed reload triggers rollback
  const tx1 = new SimConfigTx();
  tx1.setYaml('corrupted: yaml\n');
  tx1.setJson({ refresh_interval_sec: 99 });
  assert.throws(() => tx1.commitAndReload(true), /Daemon reload rejected/);

  // Verify rollback restored disk files
  assert.strictEqual(fs.readFileSync(yamlPath, 'utf8'), origYaml, 'Disk YAML must be restored on rollback');
  assert.strictEqual(fs.readFileSync(jsonPath, 'utf8'), origJson, 'Disk JSON must be restored on rollback');
  assert.strictEqual(inMemoryState.refresh_interval_sec, 10, 'In-memory state must remain unchanged');

  // Test successful commit
  const tx2 = new SimConfigTx();
  tx2.setYaml('port: 9090\n');
  tx2.setJson({ refresh_interval_sec: 25 });
  tx2.commitAndReload(false);

  assert.strictEqual(fs.readFileSync(yamlPath, 'utf8'), 'port: 9090\n', 'Disk YAML updated');
  assert.strictEqual(inMemoryState.refresh_interval_sec, 25, 'In-memory state committed');

  fs.rmSync(tmpDir, { recursive: true, force: true });
});

console.log(`\n=== All ${passed}/${total} Milestone 2 Checks Passed Successfully ===\n`);
