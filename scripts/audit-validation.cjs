const fs = require('node:fs');
const path = require('node:path');
const cp = require('node:child_process');
const root = path.resolve(__dirname, '..');
const front = path.join(root, 'frontend');
const results = [];
for (const [name, args] of [
  ['vitest', ['node_modules/vitest/vitest.mjs', 'run', '--reporter=dot']],
  ['tsc', ['node_modules/typescript/bin/tsc', '--noEmit']],
]) {
  const r = cp.spawnSync(process.execPath, args, { cwd: front, encoding: 'utf8', timeout: 90000 });
  results.push({ name, status: r.status, error: r.error?.message, stdout: r.stdout, stderr: r.stderr });
}
const ts = require(path.join(front, 'node_modules/typescript'));
const source = fs.readFileSync(path.join(front, 'src/utils/nodeParser.ts'), 'utf8');
const compiled = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS } }).outputText;
const mod = { exports: {} };
new Function('exports', 'module', compiled)(mod.exports, mod);
const { parseProxyLink, exportServerToLink } = mod.exports;
for (const link of [
  'trojan://p%40ss@example.com:443#test',
  'hysteria2://p%40ss@example.com:443?obfs=salamander&obfs-password=test#test',
  'vless://id@example.com:443?security=tls&type=ws&path=%2Fcustom&host=cdn.example.com#test',
  'trojan://password@example.com:443#bad%22name',
]) {
  const parsed = parseProxyLink(link);
  const exported = exportServerToLink({ name: parsed.name, protocol: parsed.protocol, host: parsed.raw.server, port: parsed.raw.port, raw: parsed.raw });
  results.push({ link, parsed, exported, reparsed: parseProxyLink(exported) });
}
fs.writeFileSync(path.join(__dirname, 'audit-validation-results.json'), JSON.stringify(results, null, 2));
console.log('Audit validation completed');
