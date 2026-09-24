const http = require('http');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { spawn } = require('child_process');

const PORT = 3456;
const DIST_DIR = path.join(__dirname, '..', 'frontend', 'dist');
const SCREENSHOT_DIR = path.join(__dirname, '..', 'docs', 'screenshots');

if (!fs.existsSync(SCREENSHOT_DIR)) {
  fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
}

// 1. Mock API Data
const mockStatus = {
  version: "v1.3.3",
  config_path: "/opt/etc/xkeen-route/config.json",
  router: {
    model: "Keenetic Peak (KN-4110) WBR3000UAX",
    version: "4.2.1",
    hostname: "Keenetic-Peak",
    uptime: "18д 14ч 32м"
  },
  system: {
    cpu_percent: 4,
    memory_used_mb: 218,
    memory_total_mb: 512,
    app_memory_mb: 14.8,
    app_cpu_percent: 0.7,
    core_memory_mb: 42.1,
    total_xkeen_memory_mb: 56.9
  },
  mihomo_version: "Mihomo Meta v1.19.0 (alpha-1845)",
  active_server: {
    id: "de_vless",
    name: "🇩🇪 Германия Frankfurt [VLESS-Reality]",
    protocol: "VLESS",
    host: "fra-node.vless.net",
    port: 443,
    ping_ms: 34,
    provider: "sub1",
    provider_name: "FastProxy VIP"
  },
  mihomo: { host: "127.0.0.1", port: 9090 },
  rci: { host: "127.0.0.1", port: 79 },
  failover: {
    enabled: true,
    ping_threshold_ms: 250,
    priority_server: "de_vless",
    priority_chain: ["de_vless", "nl_vless", "fi_trojan"],
    auto_restore_priority: true,
    interval_secs: 45,
    device_failover_enabled: true
  },
  refresh_interval_sec: 5,
  adblock_enabled: true
};

const mockServers = [
  { id: "de_vless", name: "🇩🇪 Германия Frankfurt [VLESS-Reality]", protocol: "VLESS", host: "fra-node.vless.net", port: 443, is_active: true, is_priority: true, flow_status: "ok", ping_ms: 34, provider: "sub1", provider_name: "FastProxy VIP" },
  { id: "nl_vless", name: "🇳🇱 Нидерланды Amsterdam [VLESS]", protocol: "VLESS", host: "ams-node.vless.net", port: 443, is_active: false, is_priority: false, flow_status: "ok", ping_ms: 38, provider: "sub1", provider_name: "FastProxy VIP" },
  { id: "fi_trojan", name: "🇫🇮 Финляндия Helsinki [Trojan]", protocol: "Trojan", host: "hel-node.trojan.net", port: 443, is_active: false, is_priority: false, flow_status: "blocked", ping_ms: 44, provider: "sub1", provider_name: "FastProxy VIP" },
  { id: "pl_vless", name: "🇵🇱 Польша Warsaw [Reality]", protocol: "VLESS", host: "waw-node.vless.net", port: 443, is_active: false, is_priority: false, flow_status: "ok", ping_ms: 49, provider: "sub1", provider_name: "FastProxy VIP" },
  { id: "se_vless", name: "🇸🇪 Швеция Stockholm [VLESS]", protocol: "VLESS", host: "sto-node.vless.net", port: 443, is_active: false, is_priority: false, flow_status: "ok", ping_ms: 52, provider: "sub1", provider_name: "FastProxy VIP" },
  { id: "tr_vless", name: "🇹🇷 Турция Istanbul [VLESS]", protocol: "VLESS", host: "ist-node.vless.net", port: 443, is_active: false, is_priority: false, flow_status: "ok", ping_ms: 68, provider: "sub2", provider_name: "Backup Stream" },
  { id: "us_ss", name: "🇺🇸 США New York [Shadowsocks]", protocol: "Shadowsocks", host: "nyc-node.ss.net", port: 8388, is_active: false, is_priority: false, flow_status: "ok", ping_ms: 108, provider: "sub2", provider_name: "Backup Stream" },
  { id: "jp_vless", name: "🇯🇵 Япония Tokyo [VLESS]", protocol: "VLESS", host: "tyo-node.vless.net", port: 443, is_active: false, is_priority: false, flow_status: "ok", ping_ms: 172, provider: "sub2", provider_name: "Backup Stream" },
  { id: "fallback_auto", name: "🛡️ Fallback (Автовыбор)", protocol: "Fallback", host: "auto", port: 0, is_active: false, is_priority: false, flow_status: "unknown", ping_ms: -1 },
  { id: "fastest_auto", name: "⚡ Fastest (Автопинг)", protocol: "URL-Test", host: "auto", port: 0, is_active: false, is_priority: false, flow_status: "unknown", ping_ms: -1 }
];

const mockProviders = [
  { id: "sub1", name: "FastProxy VIP", count: 6, vehicle_type: "HTTP", updated_at: "2026-09-24 04:00", url: "https://my-sub.net/token123" },
  { id: "sub2", name: "Backup Stream", count: 4, vehicle_type: "HTTP", updated_at: "2026-09-24 04:00", url: "https://my-backup.net/token456" }
];

const mockPolicies = [
  { id: "default", name: "Основная (Провайдер)", is_default: true },
  { id: "xkeen", name: "XKeen", is_default: false },
  { id: "direct_only", name: "Прямой доступ", is_default: false }
];

const mockDevices = [
  { mac: "04:7C:16:88:99:AA", name: "Big PC", ip: "192.168.1.100", policy: "xkeen", policy_name: "XKeen", online: true, interface: "Home", is_current_device: true, rxbytes: 56265883648, txbytes: 6442450944, speed_limit_kbps: 0, current_server: "de_vless" },
  { mac: "9C:28:F7:11:22:33", name: "Xiaomi 13 Pro", ip: "192.168.1.105", policy: "xkeen", policy_name: "XKeen", online: true, interface: "Wi-Fi 5GHz", is_current_device: false, rxbytes: 15891456000, txbytes: 1476395000, speed_limit_kbps: 0, current_server: "pl_vless" },
  { mac: "64:6C:B2:44:55:66", name: "LG Smart TV OLED 4K", ip: "192.168.1.120", policy: "xkeen", policy_name: "XKeen", online: true, interface: "Home", is_current_device: false, rxbytes: 103289000000, txbytes: 3200000000, speed_limit_kbps: 0, current_server: "nl_vless" },
  { mac: "F8:E0:79:77:88:99", name: "PlayStation 5", ip: "192.168.1.150", policy: "default", policy_name: "Основная (Провайдер)", online: true, interface: "Home", is_current_device: false, rxbytes: 152470000000, txbytes: 12800000000, speed_limit_kbps: 0, current_server: "default" },
  { mac: "A0:78:17:AA:BB:CC", name: "MacBook Air M2", ip: "192.168.1.112", policy: "xkeen", policy_name: "XKeen", online: true, interface: "Wi-Fi 5GHz", is_current_device: false, rxbytes: 24051814400, txbytes: 2899102976, speed_limit_kbps: 0, current_server: "fi_trojan" },
  { mac: "E4:5F:01:DD:EE:FF", name: "iPhone 15 Pro", ip: "192.168.1.108", policy: "xkeen", policy_name: "XKeen", online: true, interface: "Wi-Fi 5GHz", is_current_device: false, rxbytes: 9771050000, txbytes: 1100000000, speed_limit_kbps: 0, current_server: "de_vless" },
  { mac: "48:D7:05:01:02:03", name: "HomePod Mini (Кухня)", ip: "192.168.1.140", policy: "xkeen", policy_name: "XKeen", online: true, interface: "Wi-Fi 2.4GHz", is_current_device: false, rxbytes: 3435973836, txbytes: 419430400, speed_limit_kbps: 0, current_server: "default" },
  { mac: "50:EC:50:04:05:06", name: "Roborock S7 MaxV", ip: "192.168.1.180", policy: "default", policy_name: "Основная (Провайдер)", online: false, interface: "Wi-Fi 2.4GHz", is_current_device: false, rxbytes: 251658240, txbytes: 52428800, speed_limit_kbps: 0, current_server: "default" }
];

const mockAssignments = [
  { ip: "192.168.1.100", group: "AUTO-DEVICE-192.168.1.100", current_server: "de_vless" },
  { ip: "192.168.1.105", group: "AUTO-DEVICE-192.168.1.105", current_server: "pl_vless" },
  { ip: "192.168.1.120", group: "AUTO-DEVICE-192.168.1.120", current_server: "nl_vless" },
  { ip: "192.168.1.112", group: "AUTO-DEVICE-192.168.1.112", current_server: "fi_trojan" },
  { ip: "192.168.1.108", group: "AUTO-DEVICE-192.168.1.108", current_server: "de_vless" }
];

const mockDeviceRouting = {
  "192.168.1.100": { servers: ["de_vless", "nl_vless"], ping_threshold_ms: 250, auto_restore: true },
  "192.168.1.105": { servers: ["pl_vless"], ping_threshold_ms: 300, auto_restore: true },
  "192.168.1.120": { servers: ["nl_vless"], ping_threshold_ms: 300, auto_restore: true },
  "192.168.1.112": { servers: ["fi_trojan"], ping_threshold_ms: 250, auto_restore: true },
  "192.168.1.108": { servers: ["de_vless"], ping_threshold_ms: 250, auto_restore: true }
};

const mockDeviceTraffic = {
  download_total: 365284000000,
  upload_total: 27072000000,
  devices: {
    "192.168.1.100": { ip: "192.168.1.100", download_bytes: 56265883648, upload_bytes: 6442450944, active_connections: 18, active_server: "de_vless", recent_hosts: ["youtube.com", "github.com", "t.me"] },
    "192.168.1.105": { ip: "192.168.1.105", download_bytes: 15891456000, upload_bytes: 1476395000, active_connections: 8, active_server: "pl_vless", recent_hosts: ["instagram.com", "spotify.com"] },
    "192.168.1.120": { ip: "192.168.1.120", download_bytes: 103289000000, upload_bytes: 3200000000, active_connections: 6, active_server: "nl_vless", recent_hosts: ["netflix.com", "youtube.com"] },
    "192.168.1.112": { ip: "192.168.1.112", download_bytes: 24051814400, upload_bytes: 2899102976, active_connections: 12, active_server: "fi_trojan", recent_hosts: ["claude.ai", "chatgpt.com"] }
  }
};

const mockConnections = {
  downloadTotal: 48520000,
  uploadTotal: 6240000,
  connections: [
    { id: "c1", metadata: { network: "tcp", type: "HTTPS", sourceIP: "192.168.1.100", sourcePort: "54210", destinationIP: "142.250.186.206", destinationPort: "443", host: "rr2---sn-4g5ednle.googlevideo.com", dnsMode: "fake-ip" }, upload: 124000, download: 34500000, start: "2026-09-24T05:10:00Z", chains: ["🇩🇪 Германия Frankfurt", "PROXY"], rule: "DOMAIN-SUFFIX", rulePayload: "googlevideo.com" },
    { id: "c2", metadata: { network: "tcp", type: "HTTPS", sourceIP: "192.168.1.100", sourcePort: "54232", destinationIP: "140.82.121.4", destinationPort: "443", host: "github.com", dnsMode: "fake-ip" }, upload: 45000, download: 890000, start: "2026-09-24T05:12:15Z", chains: ["🇩🇪 Германия Frankfurt", "PROXY"], rule: "DOMAIN-SUFFIX", rulePayload: "github.com" },
    { id: "c3", metadata: { network: "tcp", type: "HTTPS", sourceIP: "192.168.1.112", sourcePort: "58920", destinationIP: "104.18.37.228", destinationPort: "443", host: "chatgpt.com", dnsMode: "fake-ip" }, upload: 89000, download: 2450000, start: "2026-09-24T05:11:40Z", chains: ["🇫🇮 Финляндия Helsinki", "PROXY"], rule: "DOMAIN-SUFFIX", rulePayload: "chatgpt.com" },
    { id: "c4", metadata: { network: "tcp", type: "HTTPS", sourceIP: "192.168.1.105", sourcePort: "49120", destinationIP: "157.240.247.174", destinationPort: "443", host: "instagram.com", dnsMode: "fake-ip" }, upload: 320000, download: 7600000, start: "2026-09-24T05:13:02Z", chains: ["🇵🇱 Польша Warsaw", "PROXY"], rule: "DOMAIN-SUFFIX", rulePayload: "instagram.com" },
    { id: "c5", metadata: { network: "tcp", type: "HTTPS", sourceIP: "192.168.1.120", sourcePort: "60114", destinationIP: "54.154.120.89", destinationPort: "443", host: "netflix.com", dnsMode: "fake-ip" }, upload: 11000, download: 18400000, start: "2026-09-24T05:08:44Z", chains: ["🇳🇱 Нидерланды Amsterdam", "PROXY"], rule: "DOMAIN-SUFFIX", rulePayload: "netflix.com" },
    { id: "c6", metadata: { network: "tcp", type: "HTTPS", sourceIP: "192.168.1.100", sourcePort: "54300", destinationIP: "149.154.167.50", destinationPort: "443", host: "web.telegram.org", dnsMode: "fake-ip" }, upload: 21000, download: 540000, start: "2026-09-24T05:14:00Z", chains: ["🇩🇪 Германия Frankfurt", "PROXY"], rule: "DOMAIN-SUFFIX", rulePayload: "telegram.org" }
  ]
};

const mockRules = [
  { type: "DOMAIN-SUFFIX", payload: "youtube.com", proxy: "PROXY" },
  { type: "DOMAIN-SUFFIX", payload: "googlevideo.com", proxy: "PROXY" },
  { type: "DOMAIN-SUFFIX", payload: "github.com", proxy: "PROXY" },
  { type: "DOMAIN-SUFFIX", payload: "openai.com", proxy: "PROXY" },
  { type: "DOMAIN-SUFFIX", payload: "chatgpt.com", proxy: "PROXY" },
  { type: "DOMAIN-SUFFIX", payload: "claude.ai", proxy: "PROXY" },
  { type: "DOMAIN-SUFFIX", payload: "anthropic.com", proxy: "PROXY" },
  { type: "DOMAIN-SUFFIX", payload: "instagram.com", proxy: "PROXY" },
  { type: "DOMAIN-SUFFIX", payload: "netflix.com", proxy: "PROXY" },
  { type: "DOMAIN-SUFFIX", payload: "spotify.com", proxy: "PROXY" },
  { type: "DOMAIN-KEYWORD", payload: "keenetic", proxy: "DIRECT" },
  { type: "IP-CIDR", payload: "192.168.0.0/16", proxy: "DIRECT" },
  { type: "GEOIP", payload: "RU", proxy: "DIRECT" },
  { type: "MATCH", payload: "", proxy: "PROXY" }
];

const mockHealth = {
  checks: [
    { id: "rci", name: "Keenetic RCI API", status: "ok", message: "Подключено (KN-4110 KeeneticOS 4.2.1)", latency_ms: 2 },
    { id: "mihomo", name: "Mihomo Core API", status: "ok", message: "Работает (v1.19.0 alpha-1845)", latency_ms: 1 },
    { id: "dns", name: "Smart DNS / Fake-IP", status: "ok", message: "DNS резолвер активен, DoH 1.1.1.1 / 8.8.8.8", latency_ms: 14 },
    { id: "failover", name: "Failover Watchdog", status: "ok", message: "Мониторинг активен (интервал 45 сек)", latency_ms: 0 },
    { id: "storage", name: "Хранилище Entware", status: "ok", message: "1.4 ГБ / 8.0 ГБ свободно (18% занято)" },
    { id: "memory", name: "Оперативная память", status: "ok", message: "218 МБ / 512 МБ (41% занято, 294 МБ свободно)" }
  ]
};

const mockAntigravity = {
  enabled: true,
  state: "working",
  current_route: "Google Direct Anycast / Clean Geo",
  active_ip: "142.250.186.206",
  latency_ms: 34,
  proxy_port: 53129,
  proxy_running: true,
  mode: "smart-dns",
  providers: [
    { name: "Google Cloud DNS", provider_type: "DoH", is_substituting: true, last_latency_ms: 28, resolved_ips: ["142.250.186.206", "172.217.16.206"], last_check: "2026-09-24 05:14:10" },
    { name: "Cloudflare DNS", provider_type: "DoT", is_substituting: false, last_latency_ms: 32, resolved_ips: ["1.1.1.1", "1.0.0.1"], last_check: "2026-09-24 05:14:10" }
  ],
  events: [
    { time: "05:12:00", message: "Проверка чистоты Google Geo: US (Чистый IP, Gemini & AI Studio доступны)", level: "success" },
    { time: "05:00:15", message: "Оптимизация DoH маршрутов выполнена (пинг снижен на 8 мс)", level: "info" }
  ],
  targets: ["gemini.google.com", "aistudio.google.com", "generativelanguage.googleapis.com"],
  own_proxy: ""
};

const mockSettings = {
  rci: { host: "127.0.0.1", port: 79, login: "admin", password: "••••••••", use_https: false, token: "ndm_tkn_8841a" },
  mihomo: { host: "127.0.0.1", port: 9090, secret: "", config_path: "/opt/etc/mihomo/config.yaml", device_providers: ["sub1", "sub2"] },
  failover: { enabled: true, ping_threshold_ms: 250, priority_server: "de_vless", priority_chain: ["de_vless", "nl_vless", "fi_trojan"], auto_restore_priority: true, interval_secs: 45, device_failover_enabled: true },
  refresh_interval_sec: 5,
  system: { xkeen_init: "/opt/etc/init.d/S05xkeen", backup_dir: "/opt/backups" },
  logs: { level: "info", remote_syslog: "", log_requests: true },
  auth: { enabled: false, password_hash: "", salt: "", session_secret: "" },
  notifications: { telegram_enabled: true, telegram_bot_token: "61829381:AAF_ExampleToken_xyz", telegram_chat_id: "89124125", webhook_url: "" }
};

const mockLogs = `2026-09-24 05:14:22 [INFO] XKeen Route v1.3.3 готов к работе на порту 1001
2026-09-24 05:14:23 [INFO] [RCI] Соединение с роутером Keenetic KN-4110 (KeeneticOS 4.2.1) установлено
2026-09-24 05:14:23 [INFO] [Mihomo] Конфиг /opt/etc/mihomo/config.yaml прочитан (успешно, 10 серверов)
2026-09-24 05:14:25 [INFO] [Failover] Активный сервер: 🇩🇪 Германия Frankfurt [VLESS-Reality] (пинг 34 мс < 250 мс)
2026-09-24 05:14:30 [INFO] [Per-Device] 5 устройств привязаны к персональным серверам маршрутизации
2026-09-24 05:14:35 [INFO] [SmartDNS] DNS cache warmed up (240 записей, Fake-IP режим активен)
2026-09-24 05:15:00 [INFO] [Failover] Проверка по расписанию: все узлы доступны, статус OK`;

let pollCount = 0;

const server = http.createServer((req, res) => {
  const url = new URL(req.url, `http://${req.headers.host}`);
  const pathname = url.pathname;

  const jsonOk = (data) => {
    res.writeHead(200, { 'Content-Type': 'application/json', 'Access-Control-Allow-Origin': '*' });
    res.end(JSON.stringify({ success: true, data }));
  };

  if (pathname.startsWith('/api/')) {
    const apiPath = pathname.replace(/^\/api\//, '');
    if (apiPath === 'status') return jsonOk(mockStatus);
    if (apiPath === 'system/metrics') return jsonOk(mockStatus.system);
    if (apiPath === 'auth/status') return jsonOk({ enabled: false, authenticated: true });
    if (apiPath === 'update/check') return jsonOk({ current: "v1.3.3", latest: "v1.3.3", update_available: false, notes: [] });
    if (apiPath === 'failover/events') return jsonOk({
      events: [
        { time: "05:14:22", message: "🇩🇪 Германия Frankfurt (34 мс) — в норме", switched: false },
        { time: "04:58:10", message: "Восстановление на приоритетный сервер: 🇩🇪 Германия Frankfurt (32 мс)", switched: true },
        { time: "04:52:05", message: "Превышен порог (290 мс > 250 мс) -> автопереключение на 🇳🇱 Нидерланды Amsterdam (38 мс)", switched: true },
        { time: "04:00:00", message: "Плановая синхронизация подписок Mihomo: обновлено 10 серверов", switched: false }
      ]
    });
    if (apiPath === 'traffic/poll') {
      pollCount++;
      const pDown = Math.round(11000000 + Math.sin(pollCount * 0.4) * 3500000 + Math.random() * 500000);
      const pUp = Math.round(1600000 + Math.cos(pollCount * 0.4) * 400000);
      const dDown = Math.round(3800000 + Math.sin(pollCount * 0.3) * 1200000);
      const dUp = Math.round(290000 + Math.random() * 50000);
      return jsonOk({
        direct: { down: dDown, up: dUp },
        proxy: { down: pDown, up: pUp },
        total: { down: pDown + dDown, up: pUp + dUp }
      });
    }
    if (apiPath === 'servers') return jsonOk({ servers: mockServers });
    if (apiPath === 'providers') return jsonOk({ providers: mockProviders });
    if (apiPath === 'ignore') return jsonOk({ servers: [] });
    if (apiPath === 'policies') return jsonOk({ policies: mockPolicies });
    if (apiPath === 'devices') return jsonOk({ devices: mockDevices });
    if (apiPath === 'routing') return jsonOk({ assignments: mockAssignments });
    if (apiPath === 'device-routing') return jsonOk({ routing: mockDeviceRouting, device_failover_enabled: true });
    if (apiPath === 'devices/traffic') return jsonOk(mockDeviceTraffic);
    if (apiPath === 'connections') return jsonOk(mockConnections);
    if (apiPath === 'rules') return jsonOk({ rules: mockRules });
    if (apiPath === 'rules/test') return jsonOk({
      matched_rule: "DOMAIN-SUFFIX, youtube.com, PROXY",
      rule_type: "DOMAIN-SUFFIX",
      target_group: "PROXY",
      resolved_server: "🇩🇪 Германия Frankfurt [VLESS-Reality]",
      reason: "Совпадение по суффиксу домена youtube.com"
    });
    if (apiPath === 'diagnostics/health') return jsonOk(mockHealth);
    if (apiPath === 'diagnostics/dns-test') return jsonOk({
      domain: "chatgpt.com",
      resolved_ips: ["104.18.37.228", "172.64.155.209"],
      is_poisoned: false,
      http_direct_ok: false,
      http_proxy_ok: true,
      verdict: "Домен доступен через PROXY",
      recommendation: "Маршрутизация работает корректно через узел 🇩🇪 Германия Frankfurt"
    });
    if (apiPath === 'servers/google-check') return jsonOk({
      server_name: "🇩🇪 Германия Frankfurt [VLESS-Reality]",
      is_clean: true,
      google_country: "United States (US)",
      google_domain: "google.com",
      client_ip: "142.250.186.206"
    });
    if (apiPath === 'antigravity/status') return jsonOk(mockAntigravity);
    if (apiPath === 'settings') return jsonOk(mockSettings);
    if (apiPath === 'domains') return jsonOk({ direct: ["gosuslugi.ru", "sberbank.ru", "tbank.ru", "vk.com", "kinopoisk.ru"], force: ["youtube.com", "googlevideo.com", "discord.com", "chatgpt.com", "openai.com", "claude.ai", "instagram.com", "spotify.com"] });
    if (apiPath === 'backups') return jsonOk({ backups: [{ name: "backup-2026-09-24-auto.tar.gz" }, { name: "backup-2026-09-20-manual.tar.gz" }], dir: "/opt/backups" });
    if (apiPath === 'dns/mode') return jsonOk({ enhanced_mode: "fake-ip", proxy_dns: "https://1.1.1.1/dns-query" });
    if (apiPath === 'adblock') return jsonOk({ enabled: true });
    if (apiPath === 'system/geo-info') return jsonOk({ geoip: { size: 4820000, updated_at: "2026-09-24 02:00" }, geosite: { size: 12400000, updated_at: "2026-09-24 02:00" } });
    if (apiPath === 'zapret/status') return jsonOk({ installed: true, running: true, pid: 1420 });
    if (apiPath.startsWith('logs')) return jsonOk({ text: mockLogs });
    if (apiPath === 'config-files/list') return jsonOk([
      { id: "mihomo_yaml", name: "config.yaml (Mihomo)", path: "/opt/etc/mihomo/config.yaml", syntax: "yaml" },
      { id: "xkeen_route_json", name: "config.json (Панель)", path: "/opt/etc/xkeen-route/config.json", syntax: "json" }
    ]);
    return jsonOk({});
  }

  // Static files
  let filePath = path.join(DIST_DIR, pathname === '/' ? 'index.html' : pathname);
  if (!fs.existsSync(filePath)) {
    filePath = path.join(DIST_DIR, 'index.html');
  }

  const ext = path.extname(filePath);
  const mimeTypes = {
    '.html': 'text/html; charset=utf-8',
    '.js': 'application/javascript',
    '.css': 'text/css',
    '.svg': 'image/svg+xml',
    '.json': 'application/json',
    '.png': 'image/png'
  };

  res.writeHead(200, { 'Content-Type': mimeTypes[ext] || 'application/octet-stream' });
  fs.createReadStream(filePath).pipe(res);
});

async function sleep(ms) {
  return new Promise(resolve => setTimeout(resolve, ms));
}

async function getPageDebuggerUrl(port) {
  for (let i = 0; i < 30; i++) {
    try {
      const res = await fetch(`http://127.0.0.1:${port}/json/list`);
      if (res.ok) {
        const list = await res.json();
        const page = list.find(item => item.type === 'page');
        if (page && page.webSocketDebuggerUrl) {
          return page.webSocketDebuggerUrl;
        }
      }
    } catch (e) {}
    await sleep(200);
  }
  throw new Error('Could not find page target in browser');
}

async function main() {
  await new Promise(res => server.listen(PORT, res));
  console.log(`Mock server running at http://localhost:${PORT}`);

  const chromePath = 'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe';
  const edgePath = 'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe';
  const browserBin = fs.existsSync(chromePath) ? chromePath : edgePath;

  const tmpDir = path.join(os.tmpdir(), 'cdp_scr_' + Date.now());
  fs.mkdirSync(tmpDir, { recursive: true });

  const cdpPort = 9444;
  const proc = spawn(browserBin, [
    '--headless=new',
    `--remote-debugging-port=${cdpPort}`,
    '--no-sandbox',
    '--disable-gpu',
    '--hide-scrollbars',
    '--force-device-scale-factor=1.5',
    `--user-data-dir=${tmpDir}`,
    `http://localhost:${PORT}/#dashboard`
  ]);

  try {
    const wsUrl = await getPageDebuggerUrl(cdpPort);
    console.log('Connected to Page CDP WebSocket:', wsUrl);
    const ws = new WebSocket(wsUrl);
    await new Promise((res, rej) => {
      ws.onopen = res;
      ws.onerror = rej;
    });

    let msgId = 1;
    const callbacks = new Map();
    ws.onmessage = (event) => {
      const data = JSON.parse(event.data);
      if (data.id && callbacks.has(data.id)) {
        const { resolve, reject } = callbacks.get(data.id);
        callbacks.delete(data.id);
        if (data.error) reject(new Error(data.error.message));
        else resolve(data.result);
      }
    };

    function send(method, params = {}) {
      return new Promise((resolve, reject) => {
        const id = msgId++;
        callbacks.set(id, { resolve, reject });
        ws.send(JSON.stringify({ id, method, params }));
      });
    }

    await send('Page.enable');
    await send('Runtime.enable');
    await send('Emulation.setDeviceMetricsOverride', {
      width: 1440,
      height: 940,
      deviceScaleFactor: 1.5,
      mobile: false,
    });
    await send('Emulation.setEmulatedMedia', {
      media: 'screen',
      features: [{ name: 'prefers-color-scheme', value: 'dark' }]
    });

    // Set initial localStorage
    await send('Runtime.evaluate', {
      expression: `
        localStorage.setItem('xr_theme', 'dark');
        localStorage.setItem('xr_google_geo', JSON.stringify({
          server_name: "🇩🇪 Германия Frankfurt [VLESS-Reality]",
          is_clean: true,
          google_country: "United States (US)",
          google_domain: "google.com",
          client_ip: "142.250.186.206"
        }));
      `
    });

    const screens = [
      { tab: 'dashboard', filename: 'dashboard.png', waitMs: 1500 },
      { tab: 'devices', filename: 'devices.png', waitMs: 1500 },
      { tab: 'servers', filename: 'servers.png', waitMs: 1500 },
      { tab: 'connections', filename: 'connections.png', waitMs: 1500 },
      { tab: 'rules', filename: 'rules.png', waitMs: 1500 },
      { tab: 'diagnostics', filename: 'diagnostics.png', waitMs: 1500 },
      { tab: 'google-ai', filename: 'google_ai.png', waitMs: 1500 },
      { tab: 'settings', filename: 'settings.png', waitMs: 1500 },
    ];

    for (const scr of screens) {
      console.log(`Capturing #${scr.tab} -> ${scr.filename}...`);
      await send('Page.navigate', { url: `http://localhost:${PORT}/#${scr.tab}` });
      await sleep(scr.waitMs);

      // If dashboard, populate smooth waveform SVG in traffic graph
      if (scr.tab === 'dashboard') {
        await send('Runtime.evaluate', {
          expression: `
            const ptsDirect = [];
            const ptsProxy = [];
            const w = 640, h = 130, n = 30;
            for (let i = 0; i < n; i++) {
              const x = (i / (n - 1)) * w;
              const yDirect = h - 24 - Math.sin(i * 0.45) * 16 - (i % 3) * 3;
              const yProxy = h - 70 - Math.sin(i * 0.35 + 1) * 32 - Math.cos(i * 0.7) * 10;
              ptsDirect.push({ x, y: yDirect });
              ptsProxy.push({ x, y: yProxy });
            }
            const lineD = (pts) => pts.reduce((acc, p, i) => acc + (i === 0 ? 'M' : ' L') + ' ' + p.x.toFixed(1) + ' ' + p.y.toFixed(1), '');
            const areaD = (pts) => lineD(pts) + ' L ' + w + ' ' + h + ' L 0 ' + h + ' Z';
            
            const paths = document.querySelectorAll('.traffic-graph-card svg path');
            if (paths.length >= 4) {
              paths[0].setAttribute('d', areaD(ptsProxy));
              paths[1].setAttribute('d', lineD(ptsProxy));
              paths[2].setAttribute('d', areaD(ptsDirect));
              paths[3].setAttribute('d', lineD(ptsDirect));
            }
          `
        });
        await sleep(300);
      }

      // Trigger interactive elements if needed
      if (scr.tab === 'rules') {
        await send('Runtime.evaluate', {
          expression: `
            const btn = document.querySelector('.simulator-form button[type="submit"]');
            if (btn) btn.click();
          `
        });
        await sleep(600);
      }

      if (scr.tab === 'diagnostics') {
        await send('Runtime.evaluate', {
          expression: `
            const btn = document.querySelector('.smart-dns-form button[type="submit"]');
            if (btn) btn.click();
          `
        });
        await sleep(600);
      }

      if (scr.tab === 'google-ai') {
        await send('Runtime.evaluate', {
          expression: `
            const btn = Array.from(document.querySelectorAll('button')).find(b => b.textContent.includes('Проверить статус Flow'));
            if (btn) btn.click();
          `
        });
        await sleep(600);
      }

      const shot = await send('Page.captureScreenshot', { format: 'png', captureBeyondViewport: false });
      const buf = Buffer.from(shot.data, 'base64');
      const outPath = path.join(SCREENSHOT_DIR, scr.filename);
      fs.writeFileSync(outPath, buf);
      console.log(`Saved: ${outPath} (${(buf.length / 1024).toFixed(1)} KB)`);
    }

    ws.close();
  } finally {
    proc.kill();
    server.close();
    try { fs.rmSync(tmpDir, { recursive: true, force: true }); } catch (e) {}
  }
}

main().then(() => console.log('All screenshots taken successfully!')).catch(console.error);
