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

export function getCountryFlag(name: string): string {
  if (!name) return '🌐'
  // 1. If server name already contains an emoji flag (Regional Indicator Symbols), use it!
  const flagMatch = name.match(/[\uD83C][\uDDE6-\uDDFF]{2}/)
  if (flagMatch) return flagMatch[0]

  const n = name.toLowerCase()

  // 2. Keyword matching by country, code, and major proxy hub cities
  if (n.includes('германи') || n.includes('germany') || n.includes('de ') || n.includes('[de]') || n.includes('de-') || n.includes('frankfurt') || n.includes('франкфурт') || n.includes('berlin') || n.includes('берлин')) return '🇩🇪'
  if (n.includes('финлянд') || n.includes('finland') || n.includes('fi ') || n.includes('[fi]') || n.includes('fi-') || n.includes('helsinki') || n.includes('хельсинки')) return '🇫🇮'
  if (n.includes('нидерланд') || n.includes('netherlands') || n.includes('голланд') || n.includes('holland') || n.includes('nl ') || n.includes('[nl]') || n.includes('nl-') || n.includes('amsterdam') || n.includes('амстердам')) return '🇳🇱'
  if (n.includes('сша') || n.includes('usa') || n.includes('united states') || n.includes('америк') || n.includes('us ') || n.includes('[us]') || n.includes('us-') || n.includes('washington') || n.includes('вашингтон') || n.includes('new york') || n.includes('лос-анджелес') || n.includes('chicago') || n.includes('miami') || n.includes('ashburn') || n.includes('seattle')) return '🇺🇸'
  if (n.includes('швеци') || n.includes('sweden') || n.includes('se ') || n.includes('[se]') || n.includes('se-') || n.includes('stockholm') || n.includes('стокгольм')) return '🇸🇪'
  if (n.includes('великобритан') || n.includes('united kingdom') || n.includes('англи') || n.includes('uk ') || n.includes('[uk]') || n.includes('uk-') || n.includes('gb ') || n.includes('[gb]') || n.includes('gb-') || n.includes('london') || n.includes('лондон')) return '🇬🇧'
  if (n.includes('франци') || n.includes('france') || n.includes('fr ') || n.includes('[fr]') || n.includes('fr-') || n.includes('paris') || n.includes('париж')) return '🇫🇷'
  if (n.includes('польш') || n.includes('poland') || n.includes('pl ') || n.includes('[pl]') || n.includes('pl-') || n.includes('warsaw') || n.includes('варшав')) return '🇵🇱'
  if (n.includes('эстони') || n.includes('estonia') || n.includes('ee ') || n.includes('[ee]') || n.includes('ee-') || n.includes('tallinn') || n.includes('таллин')) return '🇪🇪'
  if (n.includes('латви') || n.includes('latvia') || n.includes('lv ') || n.includes('[lv]') || n.includes('lv-') || n.includes('riga') || n.includes('риг')) return '🇱🇻'
  if (n.includes('литв') || n.includes('lithuania') || n.includes('lt ') || n.includes('[lt]') || n.includes('lt-') || n.includes('vilnius') || n.includes('вильнюс')) return '🇱🇹'
  if (n.includes('турци') || n.includes('turkey') || n.includes('tr ') || n.includes('[tr]') || n.includes('tr-') || n.includes('istanbul') || n.includes('стамбул')) return '🇹🇷'
  if (n.includes('казахстан') || n.includes('kazakhstan') || n.includes('kz ') || n.includes('[kz]') || n.includes('kz-') || n.includes('almaty') || n.includes('astana') || n.includes('алматы') || n.includes('астана')) return '🇰🇿'
  if (n.includes('япони') || n.includes('japan') || n.includes('jp ') || n.includes('[jp]') || n.includes('jp-') || n.includes('tokyo') || n.includes('токио')) return '🇯🇵'
  if (n.includes('сингапур') || n.includes('singapore') || n.includes('sg ') || n.includes('[sg]') || n.includes('sg-')) return '🇸🇬'
  if (n.includes('швейцари') || n.includes('switzerland') || n.includes('ch ') || n.includes('[ch]') || n.includes('ch-') || n.includes('zurich') || n.includes('geneva') || n.includes('цюрих') || n.includes('женева')) return '🇨🇭'
  if (n.includes('австри') || n.includes('austria') || n.includes('at ') || n.includes('[at]') || n.includes('at-') || n.includes('vienna') || n.includes('вена') || n.includes('вене')) return '🇦🇹'
  if (n.includes('чехи') || n.includes('czech') || n.includes('cz ') || n.includes('[cz]') || n.includes('cz-') || n.includes('prague') || n.includes('праг')) return '🇨🇿'
  if (n.includes('росси') || n.includes('russia') || n.includes('ru ') || n.includes('[ru]') || n.includes('ru-') || n.includes('moscow') || n.includes('москв') || n.includes('спб')) return '🇷🇺'
  if (n.includes('украин') || n.includes('ukraine') || n.includes('ua ') || n.includes('[ua]') || n.includes('ua-') || n.includes('kyiv') || n.includes('киев')) return '🇺🇦'
  if (n.includes('гонконг') || n.includes('hong kong') || n.includes('hk ') || n.includes('[hk]') || n.includes('hk-')) return '🇭🇰'
  if (n.includes('тайван') || n.includes('taiwan') || n.includes('tw ') || n.includes('[tw]') || n.includes('tw-')) return '🇹🇼'
  if (n.includes('коре') || n.includes('korea') || n.includes('kr ') || n.includes('[kr]') || n.includes('kr-') || n.includes('seoul') || n.includes('сеул')) return '🇰🇷'
  if (n.includes('канад') || n.includes('canada') || n.includes('ca ') || n.includes('[ca]') || n.includes('ca-') || n.includes('toronto') || n.includes('торонто')) return '🇨🇦'
  if (n.includes('австрали') || n.includes('australia') || n.includes('au ') || n.includes('[au]') || n.includes('au-') || n.includes('sydney') || n.includes('сидней')) return '🇦🇺'
  if (n.includes('испани') || n.includes('spain') || n.includes('es ') || n.includes('[es]') || n.includes('es-') || n.includes('madrid') || n.includes('barcelona') || n.includes('мадрид')) return '🇪🇸'
  if (n.includes('итали') || n.includes('italy') || n.includes('it ') || n.includes('[it]') || n.includes('it-') || n.includes('rome') || n.includes('milan') || n.includes('рим') || n.includes('милан')) return '🇮🇹'
  if (n.includes('норвеги') || n.includes('norway') || n.includes('no ') || n.includes('[no]') || n.includes('no-') || n.includes('oslo') || n.includes('осло')) return '🇳🇴'
  if (n.includes('дани') || n.includes('denmark') || n.includes('dk ') || n.includes('[dk]') || n.includes('dk-') || n.includes('copenhagen')) return '🇩🇰'
  if (n.includes('ирланд') || n.includes('ireland') || n.includes('ie ') || n.includes('[ie]') || n.includes('ie-') || n.includes('dublin') || n.includes('дублин')) return '🇮🇪'
  if (n.includes('грузи') || n.includes('georgia') || n.includes('ge ') || n.includes('[ge]') || n.includes('ge-') || n.includes('tbilisi') || n.includes('тбилиси')) return '🇬🇪'
  if (n.includes('армени') || n.includes('armenia') || n.includes('am ') || n.includes('[am]') || n.includes('am-') || n.includes('yerevan') || n.includes('ереван')) return '🇦🇲'
  if (n.includes('молдов') || n.includes('moldova') || n.includes('md ') || n.includes('[md]') || n.includes('md-') || n.includes('chisinau') || n.includes('кишинев')) return '🇲🇩'
  if (n.includes('оаэ') || n.includes('uae') || n.includes('emirates') || n.includes('dubai') || n.includes('дубай') || n.includes('ae ') || n.includes('[ae]') || n.includes('ae-')) return '🇦🇪'
  if (n.includes('израиль') || n.includes('israel') || n.includes('il ') || n.includes('[il]') || n.includes('il-')) return '🇮🇱'
  if (n.includes('серби') || n.includes('serbia') || n.includes('rs ') || n.includes('[rs]') || n.includes('rs-') || n.includes('belgrade') || n.includes('белград')) return '🇷🇸'
  if (n.includes('болгари') || n.includes('bulgaria') || n.includes('bg ') || n.includes('[bg]') || n.includes('bg-') || n.includes('sofia') || n.includes('софия')) return '🇧🇬'
  if (n.includes('румыни') || n.includes('romania') || n.includes('ro ') || n.includes('[ro]') || n.includes('ro-') || n.includes('bucharest') || n.includes('бухарест')) return '🇷🇴'
  if (n.includes('венгри') || n.includes('hungary') || n.includes('hu ') || n.includes('[hu]') || n.includes('hu-') || n.includes('budapest') || n.includes('будапешт')) return '🇭🇺'
  if (n.includes('словаки') || n.includes('slovakia') || n.includes('sk ') || n.includes('[sk]') || n.includes('sk-') || n.includes('bratislava') || n.includes('братислава')) return '🇸🇰'
  if (n.includes('греци') || n.includes('greece') || n.includes('gr ') || n.includes('[gr]') || n.includes('gr-') || n.includes('athens') || n.includes('афины')) return '🇬🇷'
  if (n.includes('португали') || n.includes('portugal') || n.includes('pt ') || n.includes('[pt]') || n.includes('pt-') || n.includes('lisbon') || n.includes('лиссабон')) return '🇵🇹'
  if (n.includes('инди') || n.includes('india') || n.includes('in ') || n.includes('[in]') || n.includes('in-') || n.includes('mumbai') || n.includes('delhi')) return '🇮🇳'
  if (n.includes('бразили') || n.includes('brazil') || n.includes('br ') || n.includes('[br]') || n.includes('br-') || n.includes('sao paulo')) return '🇧🇷'
  if (n.includes('аргентин') || n.includes('argentina') || n.includes('ar ') || n.includes('[ar]') || n.includes('ar-') || n.includes('buenos aires')) return '🇦🇷'

  return '🌐'
}
