export interface SystemStats {
  cpu_percent: number
  memory_used_mb: number
  memory_total_mb: number
  app_memory_mb?: number
  app_cpu_percent?: number
  core_memory_mb?: number
  total_xkeen_memory_mb?: number
}

export interface StatusInfo {
  version: string
  config_path: string
  router: { model?: string; version?: string; hostname?: string; uptime?: string } | null
  system?: SystemStats | null
  mihomo_version?: string | null
  active_server: {
    id: string
    name: string
    protocol?: string
    host?: string
    port?: number
    ping_ms: number
    provider?: string
    provider_name?: string
  } | null
  mihomo: { host: string; port: number }
  rci: { host: string; port: number }
  failover: {
    enabled: boolean
    ping_threshold_ms: number
    priority_server: string
    priority_chain?: string[]
    auto_restore_priority: boolean
    interval_secs: number
    device_failover_enabled?: boolean
  }
  refresh_interval_sec: number
}

export interface ServerInfo {
  id: string
  name: string
  protocol: string
  host: string
  port: number
  is_active: boolean
  is_priority: boolean
  is_google_ai?: boolean
  flow_status?: 'ok' | 'blocked' | 'unknown'
  ping_ms: number
  provider?: string
  provider_name?: string
}

export interface ProviderInfo {
  id: string
  name: string
  count: number
  vehicle_type: string
  updated_at?: string
  url?: string
}

export interface PolicyInfo {
  id: string
  name: string
  is_default: boolean
}

export interface DeviceInfo {
  mac: string
  name: string
  ip: string
  policy: string
  policy_name: string
  online: boolean
  interface: string
  is_current_device: boolean
  rxbytes: number
  txbytes: number
  speed_limit_kbps: number
  current_server: string
}

export interface RoutingAssignmentInfo {
  ip: string
  group: string
  current_server: string
}

export interface DeviceRoutingEntry {
  servers: string[]
  ping_threshold_ms: number
  auto_restore: boolean
}

export interface FailoverEventInfo {
  time: string
  message: string
  switched: boolean
}

export interface AppSettings {
  rci: { host: string; port: number; login: string; password: string; use_https: boolean; token: string }
  mihomo: { host: string; port: number; secret: string; config_path: string; device_providers: string[] }
  failover: { enabled: boolean; ping_threshold_ms: number; priority_server: string; priority_chain?: string[]; auto_restore_priority: boolean; interval_secs: number; device_failover_enabled: boolean }
  refresh_interval_sec: number
  system: { xkeen_init: string; backup_dir: string }
  logs: { level: string; remote_syslog: string; log_requests: boolean }
  auth?: { enabled: boolean; password_hash: string; salt: string; session_secret: string }
  notifications?: { telegram_enabled: boolean; telegram_bot_token: string; telegram_chat_id: string; webhook_url: string }
}

export interface AuthStatus {
  enabled: boolean
  authenticated: boolean
}

export interface ConfigFile {
  id: string
  name: string
  path: string
  syntax: 'yaml' | 'json' | 'shell' | 'text'
}

export interface DeviceTraffic {
  ip: string
  download_bytes: number
  upload_bytes: number
  active_connections: number
  active_server: string
  recent_hosts: string[]
}

export interface DeviceTrafficResponse {
  download_total: number
  upload_total: number
  devices: Record<string, DeviceTraffic>
}

export interface DeviceDomainRule {
  domain: string
  target: string
}

export interface SpeedtestResult {
  server_id: string
  latency_ms: number
  speed_mbps: number
  bytes_downloaded: number
  duration_secs: number
}

export type DnsEnhancedMode = 'fake-ip' | 'redir-host'

export interface DnsMode {
  enhanced_mode: DnsEnhancedMode
  proxy_dns: string
}

export function pingClass(ms: number): string {
  if (ms <= 0) return 'ping-none'
  if (ms < 50) return 'ping-good'
  if (ms <= 100) return 'ping-mid'
  return 'ping-bad'
}

export function fmtBytes(n: number): string {
  if (n < 1024) return `${n} Б`
  if (n < 1048576) return `${(n / 1024).toFixed(1)} КБ`
  if (n < 1073741824) return `${(n / 1048576).toFixed(1)} МБ`
  return `${(n / 1073741824).toFixed(2)} ГБ`
}

export function fmtSpeed(kbps: number): string {
  if (!kbps) return '—'
  if (kbps >= 1024) return `${Math.round(kbps / 1024)} Мбит/с`
  return `${kbps} Кбит/с`
}

export const SPEED_PRESETS: { label: string; value: number }[] = [
  { label: 'Без лимита', value: 0 },
  { label: '10 Мбит/с', value: 10240 },
  { label: '30 Мбит/с', value: 30720 },
  { label: '100 Мбит/с', value: 102400 },
]

export interface DnsProviderStatus {
  name: string
  provider_type: string
  is_substituting: boolean
  last_latency_ms?: number
  resolved_ips: string[]
  error?: string
  last_check?: string
}

export interface AntigravityEvent {
  time: string
  message: string
  level: 'info' | 'warn' | 'error' | 'success'
}

export interface AntigravityStatus {
  enabled: boolean
  state: 'working' | 'healing' | 'disabled' | 'error'
  current_route: string
  active_ip?: string
  latency_ms?: number
  proxy_port: number
  proxy_running: boolean
  mode: string
  providers: DnsProviderStatus[]
  events: AntigravityEvent[]
  targets: string[]
  own_proxy: string
}

export interface GoogleGeoStatus {
  is_clean: boolean
  google_lang: string
  active_server: string
  message: string
}

export type FlowStatus = 'ok' | 'blocked' | 'unknown'

export function getFlowStatus(name: string): FlowStatus {
  if (!name) return 'unknown'
  const n = name.toLowerCase()

  // 1. Узлы, где Google Flow и Gemini Labs гарантированно РАБОТАЮТ (США, Канада, Великобритания)
  if (
    n.includes('сша') ||
    n.includes('usa') ||
    n.includes('united states') ||
    n.includes('us ') ||
    n.includes('[us]') ||
    n.includes('us-') ||
    n.includes('вашингтон') ||
    n.includes('washington') ||
    n.includes('chicago') ||
    n.includes('чикаго') ||
    n.includes('miami') ||
    n.includes('майами') ||
    n.includes('seattle') ||
    n.includes('сиэтл') ||
    n.includes('лос-анджелес') ||
    n.includes('los angeles') ||
    n.includes('атланта') ||
    n.includes('atlanta') ||
    n.includes('феникс') ||
    n.includes('phoenix') ||
    n.includes('канад') ||
    n.includes('canada') ||
    n.includes('великобритан') ||
    n.includes('united kingdom') ||
    n.includes('london') ||
    n.includes('лондон')
  ) {
    return 'ok'
  }

  // 2. Узлы, которые Google Search/AI связывает с РФ и БЛОКИРУЕТ для Flow
  if (
    n.includes('росси') ||
    n.includes('russia') ||
    n.includes('ru ') ||
    n.includes('[ru]') ||
    n.includes('мобильный') ||
    n.includes('финлянд') ||
    n.includes('finland') ||
    n.includes('fi ') ||
    n.includes('[fi]') ||
    n.includes('казахстан') ||
    n.includes('kazakhstan') ||
    n.includes('беларус') ||
    n.includes('belarus') ||
    n.includes('таджикистан') ||
    n.includes('узбекистан') ||
    n.includes('азербайджан') ||
    n.includes('армени') ||
    n.includes('грузи')
  ) {
    return 'blocked'
  }

  return 'unknown'
}
