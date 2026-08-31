export interface StatusInfo {
  version: string
  config_path: string
  router: { model?: string; version?: string; hostname?: string; uptime?: string } | null
  active_server: { id: string; name: string; ping_ms: number } | null
  mihomo: { host: string; port: number }
  rci: { host: string; port: number }
  failover: {
    enabled: boolean
    ping_threshold_ms: number
    priority_server: string
    priority_chain?: string[]
    auto_restore_priority: boolean
    interval_secs: number
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
  ping_ms: number
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
}

export function pingClass(ms: number): string {
  if (ms <= 0) return 'ping-none'
  if (ms < 100) return 'ping-good'
  if (ms < 250) return 'ping-mid'
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
