import { useCallback, useEffect, useRef, useState } from 'react'
import { apiGet, apiPost, apiPut } from '../api'
import {
  pingClass,
  type AppSettings,
  type DnsMode,
  type DnsEnhancedMode,
  type ServerInfo,
  type ProviderInfo,
  type StatusInfo,
  type SwapStatus,
  type SwapSizePresetMb,
} from '../types'
import NumberInput from './NumberInput'
import ConfigEditor from './ConfigEditor'
import PresetCatalogModal from './PresetCatalogModal'
import LogsViewer from './LogsViewer'
import TaskManager from './TaskManager'

export function SwapManager({ notify }: { notify: (msg: string, isErr?: boolean) => void }) {
  const [status, setStatus] = useState<SwapStatus | null>(null)
  const [loading, setLoading] = useState(false)
  const [selectedSize, setSelectedSize] = useState<SwapSizePresetMb>(512)

  const loadStatus = useCallback(async () => {
    try {
      const data = await apiGet<SwapStatus>('system/swap/status')
      setStatus(data)
    } catch (e: any) {
      console.warn('Ошибка получения статуса Swap:', e)
    }
  }, [])

  useEffect(() => {
    loadStatus()
  }, [loadStatus])

  const handleCreate = async () => {
    setLoading(true)
    notify(`⏳ Выделение файла подкачки ${selectedSize} МБ на USB... Подождите 10-20 сек.`)
    try {
      const res = await apiPost<SwapStatus>('system/swap/create', { size_mb: selectedSize })
      setStatus(res)
      notify(`✓ Файл подкачки ${selectedSize} МБ успешно создан и активирован!`)
    } catch (err: any) {
      notify(err.message || 'Ошибка создания Swap', true)
    } finally {
      setLoading(false)
    }
  }

  const handleRemove = async () => {
    if (typeof window !== 'undefined' && !window.confirm('Отключить и удалить файл подкачки?')) return
    setLoading(true)
    try {
      const res = await apiPost<SwapStatus>('system/swap/remove', {})
      setStatus(res)
      notify('Файл подкачки отключен и удален')
    } catch (err: any) {
      notify(err.message || 'Ошибка отключения Swap', true)
    } finally {
      setLoading(false)
    }
  }

  if (!status || !status.swap_supported) return null

  const ramUsedMb = Math.max(0, status.ram_total_mb - status.ram_free_mb)
  const ramPercent = Math.min(100, Math.round((ramUsedMb / (status.ram_total_mb || 1)) * 100))
  const swapPercent =
    status.swap_total_mb > 0
      ? Math.min(100, Math.round((status.swap_used_mb / status.swap_total_mb) * 100))
      : 0

  return (
    <div className="card" style={{ marginTop: 16 }} data-testid="swap-manager-card">
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
        <h3 style={{ margin: 0 }}>🚀 Память и файл подкачки (Swap)</h3>
        <span
          className={`badge ${status.swap_active ? 'badge-success' : 'badge-muted'}`}
          data-testid="swap-status-badge"
        >
          {status.swap_active ? '🟢 Swap активен' : '⚪ Swap выключен'}
        </span>
      </div>
      <p className="muted small" style={{ margin: '8px 0 16px' }}>
        Технология защиты от вылетов ядра и нехватки ОЗУ на роутере Keenetic.
        Используется щадящий режим флешки (swappiness=10).
      </p>

      {/* Метрики памяти */}
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(240px, 1fr))', gap: 12, marginBottom: 16 }}>
        <div style={{ background: 'rgba(255,255,255,0.03)', padding: 12, borderRadius: 8 }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 13, marginBottom: 6 }}>
            <span>ОЗУ роутера</span>
            <b>{ramUsedMb} / {status.ram_total_mb} МБ ({ramPercent}%)</b>
          </div>
          <div className="progress-bar">
            <div
              className="progress-fill"
              style={{
                width: `${ramPercent}%`,
                background: ramPercent > 85 ? '#ef4444' : '#38bdf8',
              }}
            />
          </div>
          <div className="muted small" style={{ marginTop: 4 }}>
            Свободно: {status.ram_free_mb} МБ
          </div>
        </div>

        <div style={{ background: 'rgba(255,255,255,0.03)', padding: 12, borderRadius: 8 }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 13, marginBottom: 6 }}>
            <span>Файл подкачки (Swap)</span>
            <b>{status.swap_used_mb} / {status.swap_total_mb} МБ ({swapPercent}%)</b>
          </div>
          <div className="progress-bar">
            <div
              className="progress-fill"
              style={{ width: `${swapPercent}%`, background: '#22c55e' }}
            />
          </div>
          <div className="muted small" style={{ marginTop: 4 }}>
            Накопитель /opt: свободно {status.opt_disk_free_mb} МБ
          </div>
        </div>
      </div>

      {/* Управление */}
      {!status.swap_active ? (
        <div style={{ display: 'flex', gap: 12, alignItems: 'center', flexWrap: 'wrap' }}>
          <div style={{ display: 'flex', background: 'rgba(255,255,255,0.05)', borderRadius: 6, padding: 3 }}>
            {([256, 512, 1024] as const).map((sz) => (
              <button
                key={sz}
                type="button"
                className={`btn-tab ${selectedSize === sz ? 'active' : ''}`}
                style={{ padding: '6px 14px', fontSize: 12 }}
                onClick={() => setSelectedSize(sz)}
                data-testid={`swap-preset-${sz}`}
              >
                {sz} МБ {sz === 512 ? '★' : ''}
              </button>
            ))}
          </div>
          <button
            className="btn btn-primary"
            disabled={loading}
            onClick={handleCreate}
            style={{ minWidth: 200 }}
            data-testid="swap-create-button"
          >
            {loading ? '⏳ Создание (15 сек)...' : `🚀 Создать Swap ${selectedSize} МБ в 1 клик`}
          </button>
        </div>
      ) : (
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: 12, flexWrap: 'wrap' }}>
          <span className="small muted">
            Файл <code>{status.swap_file_path}</code> ({status.swap_total_mb} МБ) активен. Автозапуск настроен в <code>S01swap</code>.
          </span>
          <button
            className="btn btn-danger"
            disabled={loading}
            onClick={handleRemove}
            data-testid="swap-remove-button"
          >
            {loading ? '⏳ Удаление...' : '🗑 Отключить и удалить Swap'}
          </button>
        </div>
      )}
    </div>
  )
}

function fmtTrafficBytes(bytes: number): string {
  if (!bytes || bytes <= 0) return '0 B'
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(2)} MB`
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`
}

export function cleanServerDisplayName(name: string): string {
  if (!name) return ''
  const countriesRegex =
    /(?:^|[^а-яёa-z0-9_])(Германия|Германии|Нидерланды|Нидерландов|Великобритания|Великобритании|Англия|Англии|США|Франция|Франции|Финляндия|Финляндии|Швеция|Швеции|Польша|Польши|Казахстан|Казахстана|Турция|Турции|Сингапур|Сингапура|Россия|России|Germany|Netherlands|United Kingdom|USA|France|Finland|Sweden|Poland|Kazakhstan|Turkey|Singapore|Russia)(?=[^а-яёa-z0-9_]|$)/gi

  const cleaned = name
    .replace(/[\uD83C][\uDDE6-\uDDFF][\uD83C][\uDDE6-\uDDFF]/g, '')
    .replace(/^\[?(DE|GB|NL|US|FR|RU|SG|TR|FI|SE|PL|KZ|UA|UK)\]?\s*[-–—:]*\s*/i, '')
    .replace(/[-_](de|gb|nl|us|fr|ru|sg|tr|fi|se|pl|kz|ua)[-_]/gi, '-')
    .replace(/[-_](de|gb|nl|us|fr|ru|sg|tr|fi|se|pl|kz|ua)$/gi, '')
    .replace(countriesRegex, ' ')
    .replace(/\s*[-–—:]\s*(?=[(\[])/g, ' ')
    .replace(/[-–—:]\s*$/g, '')
    .replace(/^\s*[-–—:]/g, '')
    .replace(/\s*[-–—:]\s*[-–—:]\s*/g, ' - ')
    .replace(/\(\s*\)/g, '')
    .replace(/\[\s*\]/g, '')
    .replace(/\s{2,}/g, ' ')
    .trim()

  return cleaned.length > 0 ? cleaned : name
}

export function formatActiveServerStatus(statusText?: string, activeServer?: any, chain?: string[]): string {
  if (chain && chain.length > 0 && activeServer?.id) {
    const idx = chain.indexOf(activeServer.id)
    if (idx === 0) {
      return 'Штатно: активен основной узел #1'
    } else if (idx > 0) {
      return `Резерв #${idx + 1}: основной узел недоступен`
    }
  }
  if (!statusText) {
    return 'Штатный режим: переключения не требуются'
  }
  let s = statusText.replace(/\.?\s*В карантине.*$/i, '').trim()
  s = cleanServerDisplayName(s)
  if (s.length === 0) {
    if (activeServer?.name) {
      return `Штатно: активен узел '${cleanServerDisplayName(activeServer.name)}'`
    }
    return 'Штатный режим: переключения не требуются'
  }
  return s
}

interface Props {
  notify: (msg: string, isError?: boolean) => void
  status?: StatusInfo | null
  refresh?: () => void
}

export default function Settings({ notify, status, refresh }: Props) {
  const [settings, setSettings] = useState<AppSettings | null>(null)
  const [servers, setServers] = useState<ServerInfo[]>([])
  const [providers, setProviders] = useState<ProviderInfo[]>([])
  const [saving, setSaving] = useState(false)
  const [directDomains, setDirectDomains] = useState('')
  const [forceDomains, setForceDomains] = useState('')
  const [autoCdns, setAutoCdns] = useState<string[]>([])
  const [cdnsCollapsed, setCdnsCollapsed] = useState(true)
  const [savingDomains, setSavingDomains] = useState(false)
  const [scanningCdn, setScanningCdn] = useState(false)
  const [domainsRevision, setDomainsRevision] = useState<string | undefined>(undefined)
  const [failoverLastPolled, setFailoverLastPolled] = useState<string>(() => new Date().toLocaleTimeString())

  useEffect(() => {
    if (status) {
      setFailoverLastPolled(new Date().toLocaleTimeString())
    }
  }, [status])

  // --- Сводка сетевого трафика ---
  const [trafficSummary, setTrafficSummary] = useState<{
    direct_bytes: number
    proxy_bytes: number
    whitelist_bytes: number
    whitelist_active: boolean
  }>({
    direct_bytes: 0,
    proxy_bytes: 0,
    whitelist_bytes: 0,
    whitelist_active: false,
  })

  useEffect(() => {
    let active = true
    const loadTraffic = async () => {
      try {
        const data = await apiGet<any>('traffic/poll')
        if (active && data) {
          setTrafficSummary({
            direct_bytes: data.direct_bytes ?? data.direct?.bytes ?? 0,
            proxy_bytes: data.proxy_bytes ?? data.proxy?.bytes ?? 0,
            whitelist_bytes: data.whitelist_bytes ?? data.whitelist?.bytes ?? 0,
            whitelist_active: Boolean(data.whitelist_active ?? data.whitelist?.active),
          })
        }
      } catch {}
    }
    loadTraffic()
    const timer = setInterval(loadTraffic, 5000)
    return () => {
      active = false
      clearInterval(timer)
    }
  }, [])

  const handleResetTraffic = async (target: string, label: string) => {
    if (!window.confirm(`Вы действительно хотите сбросить накопленный счетчик ${label} в 0?`)) {
      return
    }
    try {
      const res = await apiPost<{ status: string; direct_bytes?: number; proxy_bytes?: number; whitelist_bytes?: number }>(
        `traffic/reset?target=${target}`,
        {}
      )
      notify(`Счётчик ${label} успешно сброшен`)
      setTrafficSummary(prev => ({
        ...prev,
        direct_bytes: res.direct_bytes !== undefined ? res.direct_bytes : (target === 'direct' ? 0 : prev.direct_bytes),
        proxy_bytes: res.proxy_bytes !== undefined ? res.proxy_bytes : (target === 'vps' ? 0 : prev.proxy_bytes),
        whitelist_bytes: res.whitelist_bytes !== undefined ? res.whitelist_bytes : (target === 'whitelist' ? 0 : prev.whitelist_bytes),
      }))
    } catch (e: any) {
      notify(`Ошибка сброса: ${e?.message || e}`, true)
    }
  }

  // Модальные окна
  const [configEditorOpen, setConfigEditorOpen] = useState(false)
  const [presetCatalogOpen, setPresetCatalogOpen] = useState(false)

  // --- DNS режим ---
  const [dnsMode, setDnsMode] = useState<DnsEnhancedMode>('fake-ip')
  const [dnsModeBusy, setDnsModeBusy] = useState(false)

  // --- Безопасность и пароль ---
  const [authEnabled, setAuthEnabled] = useState(false)
  const [currentPassword, setCurrentPassword] = useState('')
  const [newPassword, setNewPassword] = useState('')
  const [confirmPassword, setConfirmPassword] = useState('')
  const [authSaving, setAuthSaving] = useState(false)

  // --- Оповещения (Telegram / Webhook) ---
  const [telegramEnabled, setTelegramEnabled] = useState(false)
  const [telegramBotToken, setTelegramBotToken] = useState('')
  const [telegramChatId, setTelegramChatId] = useState('')
  const [webhookUrl, setWebhookUrl] = useState('')
  const [notifTesting, setNotifTesting] = useState(false)
  const [notifSaving, setNotifSaving] = useState(false)
  const [activeNotifHelp, setActiveNotifHelp] = useState<'token' | 'chatId' | 'webhook' | null>(null)

  // Валидация настроек Telegram
  const cleanNotifToken = telegramBotToken.trim().replace(/^['"]|['"]$/g, '')
  const cleanNotifChatId = telegramChatId.trim().replace(/^['"]|['"]$/g, '')

  // Сохраняем введённые данные в localStorage, чтобы они не терялись при обновлении страницы или панели
  useEffect(() => {
    if (cleanNotifToken && cleanNotifToken !== '******') {
      try { localStorage.setItem('xkeen_tg_bot_token', cleanNotifToken); } catch {}
    }
    if (cleanNotifChatId) {
      try { localStorage.setItem('xkeen_tg_chat_id', cleanNotifChatId); } catch {}
    }
  }, [cleanNotifToken, cleanNotifChatId]);

  let notifTokenWarning: string | null = null
  if (cleanNotifToken && cleanNotifToken !== '******') {
    if (cleanNotifToken.startsWith('@') || cleanNotifToken.startsWith('http://') || cleanNotifToken.startsWith('https://') || cleanNotifToken.startsWith('t.me/')) {
      notifTokenWarning = 'Введена ссылка или юзернейм вместо токена. Скопируйте токен целиком из @BotFather (вид: 123456789:ABCdef...)'
    } else {
      const parts = cleanNotifToken.split(':')
      if (parts.length !== 2 || !parts[0] || !parts[1] || !/^\d+$/.test(parts[0])) {
        notifTokenWarning = 'Токен неполный! В начале обязательно должен быть числовой ID бота и двоеточие (например: 8959891087:AAHW...)'
      }
    }
  }

  let notifChatIdWarning: string | null = null
  if (cleanNotifChatId) {
    const botIdFromToken = cleanNotifToken.includes(':') ? cleanNotifToken.split(':')[0] : ''
    if (botIdFromToken && cleanNotifChatId === botIdFromToken) {
      notifChatIdWarning = `⚠️ Вы указали ID самого бота (${cleanNotifChatId})! Бот не может писать сам себе. Получите ваш личный Chat ID через @userinfobot`
    } else if (cleanNotifChatId.startsWith('@')) {
      notifChatIdWarning = 'Личный Chat ID должен быть числовым (получите через @userinfobot). @username работает только для публичных каналов'
    } else if (!/^-?\d+$/.test(cleanNotifChatId)) {
      notifChatIdWarning = 'Chat ID должен состоять только из цифр (для групп допустим знак минус, например -100...)'
    }
  }

  // --- Сервис XKeen ---
  const [svcBusy, setSvcBusy] = useState('')
  // --- Бэкапы ---
  const [backups, setBackups] = useState<string[]>([])
  const [backupDir, setBackupDir] = useState('')
  const [backupBusy, setBackupBusy] = useState(false)
  const backupFileRef = useRef<HTMLInputElement>(null)

  // --- Автосохранение failover ---
  const [autoSaveStatus, setAutoSaveStatus] = useState<'idle' | 'saving' | 'saved' | 'error'>('idle')
  const initialFailoverRef = useRef<string | null>(null)
  const autoSaveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const savedTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  // --- Пресеты Failover слотов ---
  const [selectedPresetId, setSelectedPresetId] = useState<string | null>(null)
  const [newPresetName, setNewPresetName] = useState('')
  const [showNewPresetInput, setShowNewPresetInput] = useState(false)

  // --- AdBlock ---
  const [adblockEnabled, setAdblockEnabled] = useState(false)
  const [adblockBusy, setAdblockBusy] = useState(false)

  // --- GeoIP & GeoSite ---
  interface GeoInfo {
    geoip: { size: number; updated_at: string }
    geosite: { size: number; updated_at: string }
  }
  const [geoInfo, setGeoInfo] = useState<GeoInfo | null>(null)
  const [geoUpdating, setGeoUpdating] = useState(false)

  // --- Живой посекундный отсчет Canary-теста ---
  const canaryInfo = status?.failover?.canary
  const [canaryCountdown, setCanaryCountdown] = useState<number | null>(null)

  useEffect(() => {
    if (canaryInfo?.active && typeof canaryInfo.remaining_secs === 'number') {
      setCanaryCountdown(canaryInfo.remaining_secs)
    } else {
      setCanaryCountdown(null)
    }
  }, [canaryInfo?.active, canaryInfo?.remaining_secs, canaryInfo?.target_id])

  useEffect(() => {
    if (canaryCountdown === null || canaryCountdown <= 0) return
    const timer = setInterval(() => {
      setCanaryCountdown((prev) => (prev !== null && prev > 0 ? prev - 1 : 0))
    }, 1000)
    return () => clearInterval(timer)
  }, [canaryCountdown !== null && canaryCountdown > 0])

  const loadQuickWins = useCallback(async () => {
    try {
      const adb = await apiGet<{ enabled: boolean }>('adblock')
      setAdblockEnabled(adb.enabled)
    } catch {}
    try {
      const geo = await apiGet<GeoInfo>('system/geo-info')
      setGeoInfo(geo)
    } catch {}
  }, [])

  const handleToggleAdblock = async (enabled: boolean) => {
    setAdblockBusy(true)
    try {
      const res = await apiPost<{ enabled: boolean }>('adblock/toggle', { enabled })
      setAdblockEnabled(res.enabled)
      notify(res.enabled ? 'Блокировка рекламы на роутере включена' : 'Блокировка рекламы отключена')
      if (refresh) refresh()
    } catch (e: any) {
      notify('Ошибка: ' + e.message, true)
    } finally {
      setAdblockBusy(false)
    }
  }

  const handleUpdateGeo = async () => {
    setGeoUpdating(true)
    try {
      await apiPost<{ success: boolean }>('system/geo-update')
      notify('Базы GeoIP и GeoSite успешно обновлены')
      loadQuickWins()
    } catch (e: any) {
      notify('Ошибка обновления баз: ' + e.message, true)
    } finally {
      setGeoUpdating(false)
    }
  }


  useEffect(() => {
    loadQuickWins()
    apiGet<AppSettings>('settings').then((s) => {
      setSettings(s)
      setAuthEnabled(s.auth?.enabled ?? false)
      setTelegramEnabled(s.notifications?.telegram_enabled ?? false)
      const token = s.notifications?.telegram_bot_token ?? ''
      const chatId = s.notifications?.telegram_chat_id ?? ''
      let cachedToken = ''
      let cachedChatId = ''
      try {
        cachedToken = localStorage.getItem('xkeen_tg_bot_token') ?? ''
        cachedChatId = localStorage.getItem('xkeen_tg_chat_id') ?? ''
      } catch {}
      setTelegramBotToken(token && token !== '******' ? token : (cachedToken || token))
      setTelegramChatId(chatId || cachedChatId)
      setWebhookUrl(s.notifications?.webhook_url ?? '')
    }).catch((e) => notify(e instanceof Error ? e.message : 'Ошибка', true))

    apiGet<{ servers: ServerInfo[] }>('servers')
      .then((d) => setServers(d.servers))
      .catch(() => {})
    apiGet<{ providers: ProviderInfo[] }>('providers')
      .then((d) => setProviders(d.providers || []))
      .catch(() => {})
    apiGet<{ direct: string[]; force: string[]; auto_cdns?: string[]; revision?: string }>('domains')
      .then((d) => {
        setDirectDomains(d.direct.join('\n'))
        setForceDomains(d.force.join('\n'))
        if (d.auto_cdns) setAutoCdns(d.auto_cdns)
        if (d.revision) setDomainsRevision(d.revision)
      })
      .catch(() => {})
    apiGet<DnsMode>('dns/mode')
      .then((d) => setDnsMode(d.enhanced_mode))
      .catch(() => {})
  }, [notify])

  // Синхронизация актуальных пингов серверов с глобальным статусом панели (Failover Priority Chain & Dashboard)
  useEffect(() => {
    if (status?.failover?.priority_chain_pings) {
      const pings = status.failover.priority_chain_pings
      setServers((prev) => {
        let changed = false
        const next = prev.map((s) => {
          const updated = pings[s.id]
          if (updated !== undefined && updated !== s.ping_ms) {
            changed = true
            return { ...s, ping_ms: updated }
          }
          return s
        })
        return changed ? next : prev
      })
    }
  }, [status?.failover?.priority_chain_pings])

  // Ускоренный поллинг статуса (1.8с) во вкладке Настройки / Failover
  useEffect(() => {
    if (!refresh) return
    const interval = setInterval(refresh, 1800)
    return () => clearInterval(interval)
  }, [refresh])

  const loadBackups = useCallback(async () => {
    try {
      const d = await apiGet<{ backups: { name: string }[]; dir: string }>('backups')
      setBackups(d.backups.map((b) => b.name))
      setBackupDir(d.dir)
    } catch {
      /* бэкапы не критичны для загрузки страницы */
    }
  }, [])

  useEffect(() => {
    loadBackups()
  }, [loadBackups])

  const failoverJson = settings ? JSON.stringify(settings.failover) : ''

  useEffect(() => {
    if (!settings) return

    // Инициализация при первой загрузке: фиксируем начальное состояние без сохранения
    if (initialFailoverRef.current === null) {
      initialFailoverRef.current = failoverJson
      return
    }

    // Если настройки failover не менялись, ничего не сохраняем
    if (failoverJson === initialFailoverRef.current) {
      return
    }

    if (autoSaveTimerRef.current) {
      clearTimeout(autoSaveTimerRef.current)
    }

    setAutoSaveStatus('saving')

    autoSaveTimerRef.current = setTimeout(async () => {
      try {
        const currentFailover = JSON.parse(failoverJson)
        await apiPut('settings', {
          failover: currentFailover,
        })
        initialFailoverRef.current = failoverJson
        setAutoSaveStatus('saved')
        if (savedTimerRef.current) clearTimeout(savedTimerRef.current)
        savedTimerRef.current = setTimeout(() => {
          setAutoSaveStatus('idle')
        }, 3000)
        refresh?.()
      } catch (e) {
        setAutoSaveStatus('error')
        notify(e instanceof Error ? e.message : 'Ошибка автосохранения Failover', true)
      }
    }, 400)

    return () => {
      if (autoSaveTimerRef.current) {
        clearTimeout(autoSaveTimerRef.current)
      }
    }
  }, [failoverJson, refresh, notify])

  if (!settings) return <section className="card"><p className="muted">Загрузка…</p></section>

  const patch = (fn: (s: AppSettings) => void) => {
    const copy: AppSettings = JSON.parse(JSON.stringify(settings))
    fn(copy)
    setSettings(copy)
  }

  const toggleFailoverEnabled = async (enabled: boolean) => {
    const copy: AppSettings = JSON.parse(JSON.stringify(settings))
    copy.failover.enabled = enabled
    setSettings(copy)
    initialFailoverRef.current = JSON.stringify(copy.failover)
    if (autoSaveTimerRef.current) clearTimeout(autoSaveTimerRef.current)
    setAutoSaveStatus('saving')
    try {
      await apiPost('settings/priority', {
        server_ids: copy.failover.priority_chain ?? [],
        enabled,
      })
      await apiPut('settings', { failover: copy.failover })
      setAutoSaveStatus('saved')
      notify(enabled ? 'Failover включён' : 'Failover выключен')
      if (savedTimerRef.current) clearTimeout(savedTimerRef.current)
      savedTimerRef.current = setTimeout(() => setAutoSaveStatus('idle'), 3000)
      refresh?.()
    } catch (e) {
      setAutoSaveStatus('error')
      patch((s) => (s.failover.enabled = !enabled))
      notify(e instanceof Error ? e.message : 'Ошибка переключения failover', true)
    }
  }

  const save = async () => {
    setSaving(true)
    try {
      const updatedSettings = settings ? {
        ...settings,
        notifications: {
          ...settings.notifications,
          telegram_enabled: telegramEnabled,
          telegram_bot_token: telegramBotToken,
          telegram_chat_id: telegramChatId,
          webhook_url: webhookUrl,
        }
      } : settings
      await apiPut('settings', updatedSettings)
      if (updatedSettings) setSettings(updatedSettings)
      notify('Настройки сохранены')
      refresh?.()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка сохранения', true)
    } finally {
      setSaving(false)
    }
  }

  const testCheck = async () => {
    try {
      const data = await apiPost<{ message: string }>('failover/check')
      notify(data.message)
      refresh?.()
      apiGet<{ servers: ServerInfo[] }>('servers').then((d) => setServers(d.servers)).catch(() => {})
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка проверки', true)
    }
  }

  // --- Глобальная цепочка приоритетов ---
  const chain = settings?.failover.priority_chain ?? (settings?.failover.priority_server ? [settings.failover.priority_server] : [])

  const activePresets = settings?.failover?.presets && settings.failover.presets.length > 0
    ? settings.failover.presets
    : [
        { id: 'preset_default', name: '⚡ Основной', chain: [] },
        { id: 'preset_gaming', name: '🎮 Гейминг', chain: [] },
        { id: 'preset_streaming', name: '🎬 Стриминг', chain: [] },
      ]

  const applyPresetSlot = (slot: { id: string; name: string; chain: string[] }) => {
    setSelectedPresetId(slot.id)
    patch((s) => {
      s.failover.priority_chain = [...slot.chain]
    })
    notify(`Применен пресет: ${slot.name}`)
  }

  const saveCurrentToPreset = (slotId: string) => {
    patch((s) => {
      if (!s.failover.presets || s.failover.presets.length === 0) {
        s.failover.presets = activePresets.map((p) => ({ ...p }))
      }
      const target = s.failover.presets.find((x) => x.id === slotId)
      if (target) {
        target.chain = [...chain]
      }
    })
    const pName = activePresets.find((x) => x.id === slotId)?.name || slotId
    notify(`Текущая цепочка (${chain.length} узлов) сохранена в пресет «${pName}»`)
  }

  const createNewPresetSlot = () => {
    const name = newPresetName.trim()
    if (!name) return
    const newId = 'preset_' + Date.now()
    patch((s) => {
      const currentList = s.failover.presets && s.failover.presets.length > 0
        ? [...s.failover.presets]
        : activePresets.map((p) => ({ ...p }))
      currentList.push({ id: newId, name, chain: [...chain] })
      s.failover.presets = currentList
    })
    setSelectedPresetId(newId)
    setNewPresetName('')
    setShowNewPresetInput(false)
    notify(`Создан слот пресета «${name}»`)
  }

  const deletePresetSlot = (slotId: string) => {
    patch((s) => {
      if (!s.failover.presets) return
      s.failover.presets = s.failover.presets.filter((p) => p.id !== slotId)
    })
    if (selectedPresetId === slotId) setSelectedPresetId(null)
    notify('Слот пресета удален')
  }

  const chainMove = (idx: number, dir: -1 | 1) => {
    const next = [...chain]
    const j = idx + dir
    if (j < 0 || j >= next.length) return
    ;[next[idx], next[j]] = [next[j], next[idx]]
    patch((s) => (s.failover.priority_chain = next))
  }

  const saveDomains = async () => {
    setSavingDomains(true)
    try {
      const data = await apiPost<{
        direct: number
        force: number
        auto_cdns?: string[]
        overridden_ips?: number
        revision?: string
      }>('domains', {
        direct: directDomains.split('\n'),
        force: forceDomains.split('\n'),
        expected_revision: domainsRevision,
      })
      if (data.auto_cdns) setAutoCdns(data.auto_cdns)
      if (data.revision) setDomainsRevision(data.revision)
      const cdnMsg = data.auto_cdns && data.auto_cdns.length > 0 ? `, CDN: +${data.auto_cdns.length}` : ''
      const ipMsg = data.overridden_ips ? `, IP: ${data.overridden_ips}` : ''
      notify(`Домены сохранены: напрямую ${data.direct}, через прокси ${data.force}${cdnMsg}${ipMsg}`)
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка сохранения доменов', true)
    } finally {
      setSavingDomains(false)
    }
  }

  const scanCdnManual = async () => {
    setScanningCdn(true)
    try {
      const data = await apiPost<{
        auto_cdns?: string[]
        overridden_ips?: number
        message?: string
      }>('domains/scan-cdn', {})
      if (data.auto_cdns) setAutoCdns(data.auto_cdns)
      notify(`✓ Сканирование CDN завершено: найдено CDN: ${data.auto_cdns?.length ?? 0}, IP: ${data.overridden_ips ?? 0}`)
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка сканирования CDN', true)
    } finally {
      setScanningCdn(false)
    }
  }

  // --- Сервис XKeen ---
  const svc = async (action: string) => {
    setSvcBusy(action)
    try {
      const data = await apiPost<{ stdout: string; stderr: string }>('xkeen/service', { action })
      const out = (data.stdout || data.stderr || '').trim()
      notify(`XKeen ${action}: ${out || 'готово'}`)
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка', true)
    } finally {
      setSvcBusy('')
    }
  }

  const createBackup = async () => {
    setBackupBusy(true)
    try {
      const d = await apiPost<{ name: string }>('backups', {})
      notify(`Бэкап создан: ${d.name}`)
      loadBackups()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка', true)
    } finally {
      setBackupBusy(false)
    }
  }

  const handleImportBackup = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0]
    if (!file) return
    setBackupBusy(true)
    try {
      const text = await file.text()
      let payload: any
      try {
        payload = JSON.parse(text)
      } catch {
        throw new Error('Файл не является корректным JSON/xkbak архивом')
      }
      if (!payload || typeof payload !== 'object' || !payload.files || typeof payload.files !== 'object') {
        throw new Error('Некорректный формат архива бэкапа: отсутствует секция files')
      }
      if (!payload.name) {
        payload.name = file.name.replace(/\.[^/.]+$/, '')
      }
      const data = await apiPost<{ imported: string }>('backups/import', payload)
      notify(`Бэкап '${data.imported || file.name}' успешно импортирован`)
      loadBackups()
    } catch (err) {
      notify(err instanceof Error ? err.message : 'Ошибка импорта бэкапа', true)
    } finally {
      setBackupBusy(false)
      if (backupFileRef.current) backupFileRef.current.value = ''
    }
  }

  const restoreBackup = async (name: string) => {
    if (!confirm(`Восстановить конфиги из ${name}? Текущие config.yaml и config.json будут перезаписаны.`)) return
    setBackupBusy(true)
    try {
      await apiPost('backups/restore', { name })
      notify(`Восстановлено из ${name}`)
      loadBackups()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка', true)
    } finally {
      setBackupBusy(false)
    }
  }

  const deleteBackup = async (name: string) => {
    if (!confirm(`Удалить бэкап ${name}?`)) return
    setBackupBusy(true)
    try {
      await apiPost('backups/delete', { name })
      notify(`Бэкап ${name} удалён`)
      loadBackups()
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка', true)
    } finally {
      setBackupBusy(false)
    }
  }

  const handleSetDnsMode = async (mode: DnsEnhancedMode) => {
    setDnsModeBusy(true)
    try {
      await apiPost('dns/mode', { enhanced_mode: mode })
      setDnsMode(mode)
      notify(`DNS режим переключен на ${mode === 'fake-ip' ? 'Fake-IP' : 'Redir-Host'}`)
    } catch (err) {
      notify(err instanceof Error ? err.message : 'Ошибка переключения DNS', true)
    } finally {
      setDnsModeBusy(false)
    }
  }

  const handleSaveAuth = async () => {
    if (authEnabled && newPassword && newPassword !== confirmPassword) {
      notify('Пароли не совпадают', true)
      return
    }
    setAuthSaving(true)
    try {
      await apiPost('auth/change-password', {
        enabled: authEnabled,
        current_password: currentPassword || undefined,
        new_password: newPassword,
      })
      notify(authEnabled ? 'Защита паролем сохранена' : 'Авторизация отключена')
      setCurrentPassword('')
      setNewPassword('')
      setConfirmPassword('')
      refresh?.()
    } catch (err) {
      notify(err instanceof Error ? err.message : 'Ошибка сохранения настроек безопасности', true)
    } finally {
      setAuthSaving(false)
    }
  }

  const handleSaveNotifications = async () => {
    setNotifSaving(true)
    try {
      await apiPut('settings', {
        notifications: {
          telegram_enabled: telegramEnabled,
          telegram_bot_token: telegramBotToken.trim(),
          telegram_chat_id: telegramChatId.trim(),
          webhook_url: webhookUrl.trim(),
        },
      })
      notify('Настройки оповещений сохранены')
      refresh?.()
    } catch (err) {
      notify(err instanceof Error ? err.message : 'Ошибка сохранения оповещений', true)
    } finally {
      setNotifSaving(false)
    }
  }

  const handleTestNotification = async () => {
    if (telegramEnabled) {
      if (notifTokenWarning) {
        notify(notifTokenWarning, true)
        return
      }
      if (notifChatIdWarning) {
        notify(notifChatIdWarning, true)
        return
      }
    }
    setNotifTesting(true)
    try {
      const tokenForTest = cleanNotifToken === '******' ? undefined : (cleanNotifToken || undefined);
      const res = await apiPost<{ message?: string; results?: string[] }>('notifications/test', {
        telegram_bot_token: tokenForTest,
        telegram_chat_id: cleanNotifChatId || undefined,
        webhook_url: webhookUrl.trim() || undefined,
      })
      const results = res.results || []
      const hasError = results.some((r) => r.includes('❌'))
      const summary = results.join('\n') || res.message || 'Тестовое оповещение успешно отправлено'
      notify(summary, hasError)
    } catch (err) {
      notify(err instanceof Error ? err.message : 'Ошибка отправки тестового сообщения', true)
    } finally {
      setNotifTesting(false)
    }
  }

  const handleApplyPreset = (domains: string[], target: 'force' | 'direct') => {
    if (target === 'force') {
      const existing = new Set(forceDomains.split('\n').map((d) => d.trim()).filter(Boolean))
      domains.forEach((d) => existing.add(d))
      setForceDomains(Array.from(existing).join('\n'))
      notify(`Добавлено ${domains.length} доменов в 'Через прокси'`)
    } else {
      const existing = new Set(directDomains.split('\n').map((d) => d.trim()).filter(Boolean))
      domains.forEach((d) => existing.add(d))
      setDirectDomains(Array.from(existing).join('\n'))
      notify(`Добавлено ${domains.length} доменов в 'Напрямую'`)
    }
  }

  return (
    <div className="settings-container">
      {/* ПАНЕЛЬ БЫСТРЫХ ИНСТРУМЕНТОВ */}
      <div className="card settings-toolbar-card" style={{ display: 'flex', gap: 12, alignItems: 'center', justifyContent: 'space-between', flexWrap: 'wrap', padding: '12px 18px', background: 'linear-gradient(90deg, rgba(0, 211, 242, 0.08) 0%, rgba(43, 127, 255, 0.05) 100%)', border: '1px solid rgba(0, 211, 242, 0.2)' }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
          <span style={{ fontSize: 20 }}>🛠️</span>
          <div>
            <b style={{ fontSize: 14 }}>Инструменты конфигурации</b>
            <div className="muted small">Прямой доступ к файлам конфигов (Web-Editor) и каталогу проверенных правил</div>
          </div>
        </div>
        <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
          <button type="button" className="btn primary" onClick={() => setConfigEditorOpen(true)} title="Открыть редактор config.yaml и других файлов">
            📝 Редактор конфигов
          </button>
          <button type="button" className="btn" onClick={() => setPresetCatalogOpen(true)} title="Каталог готовых пресетов доменов">
            ✨ Каталог пресетов
          </button>
          <button
            type="button"
            className="btn"
            onClick={() => document.getElementById('task-manager-section')?.scrollIntoView({ behavior: 'smooth' })}
            title="Перейти к диспетчеру задач (htop) и мониторингу процессов"
          >
            📊 Диспетчер задач (htop)
          </button>
        </div>
      </div>

      {/* СВОДКА СЕТЕВОГО ТРАФИКА */}
      <section className="card" style={{ marginBottom: 16 }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 12 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <span style={{ fontSize: 18 }}>📊</span>
            <h3 style={{ margin: 0, fontSize: 15 }}>Сводка сетевого трафика</h3>
          </div>
          {trafficSummary.whitelist_active && (
            <span className="badge" style={{ background: 'rgba(168, 85, 247, 0.2)', color: '#c084fc', border: '1px solid rgba(168, 85, 247, 0.4)', fontSize: 11, fontWeight: 600 }}>
              🟣 Белые списки активны
            </span>
          )}
        </div>
        <p className="muted small" style={{ margin: '0 0 14px' }}>
          Накопленный объем трафика по направлениям маршрутизации. Сохраняется между перезагрузками.
        </p>
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(220px, 1fr))', gap: 12 }}>
          {/* Прямой трафик */}
          <div style={{ background: 'rgba(255,255,255,0.02)', border: '1px solid var(--border)', borderRadius: 10, padding: 12 }}>
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 6 }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 6, color: '#4ade80', fontSize: 12.5, fontWeight: 600 }}>
                <span>🟢</span>
                <span>Прямой трафик</span>
              </div>
              <button
                type="button"
                className="btn-icon-subtle"
                title="Сбросить счетчик прямого трафика"
                onClick={() => handleResetTraffic('direct', 'прямого трафика')}
                style={{ fontSize: 11, padding: '2px 7px', background: 'rgba(255,255,255,0.04)', border: '1px solid var(--border)', borderRadius: 6, cursor: 'pointer', color: 'var(--muted)' }}
              >
                🔄 Сброс
              </button>
            </div>
            <div style={{ fontSize: 20, fontWeight: 700, color: 'var(--text-bright)' }}>
              {fmtTrafficBytes(trafficSummary.direct_bytes)}
            </div>
            <div className="muted small" style={{ marginTop: 2, fontSize: 11 }}>
              Мимо прокси
            </div>
          </div>

          {/* VPS трафик (Безлимит) */}
          <div style={{ background: 'rgba(255,255,255,0.02)', border: '1px solid var(--border)', borderRadius: 10, padding: 12 }}>
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 6 }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 6, color: '#38bdf8', fontSize: 12.5, fontWeight: 600 }}>
                <span>🔵</span>
                <span>VPS трафик (Безлимит)</span>
              </div>
              <button
                type="button"
                className="btn-icon-subtle"
                title="Сбросить счетчик VPS трафика"
                onClick={() => handleResetTraffic('vps', 'VPS трафика')}
                style={{ fontSize: 11, padding: '2px 7px', background: 'rgba(255,255,255,0.04)', border: '1px solid var(--border)', borderRadius: 6, cursor: 'pointer', color: '#38bdf8' }}
              >
                🔄 Сброс
              </button>
            </div>
            <div style={{ fontSize: 20, fontWeight: 700, color: 'var(--text-bright)' }}>
              {fmtTrafficBytes(trafficSummary.proxy_bytes)}
            </div>
            <div className="muted small" style={{ marginTop: 2, fontSize: 11 }}>
              Обычные серверы
            </div>
          </div>

          {/* Туннель белых списков */}
          <div style={{ background: trafficSummary.whitelist_active ? 'rgba(168, 85, 247, 0.08)' : 'rgba(255,255,255,0.02)', border: trafficSummary.whitelist_active ? '1px solid rgba(168, 85, 247, 0.4)' : '1px solid var(--border)', borderRadius: 10, padding: 12 }}>
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 6 }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 6, color: '#c084fc', fontSize: 12.5, fontWeight: 600 }}>
                <span>🟣</span>
                <span>Туннель белых списков</span>
              </div>
              <button
                type="button"
                className="btn-icon-subtle"
                title="Сбросить счетчик трафика туннеля белых списков"
                onClick={() => handleResetTraffic('whitelist', 'трафика Белых списков (БС)')}
                style={{ fontSize: 11, padding: '2px 7px', background: 'rgba(168, 85, 247, 0.12)', border: '1px solid rgba(168, 85, 247, 0.4)', borderRadius: 6, cursor: 'pointer', color: '#c084fc' }}
              >
                🔄 Сброс
              </button>
            </div>
            <div style={{ fontSize: 20, fontWeight: 700, color: 'var(--text-bright)' }}>
              {fmtTrafficBytes(trafficSummary.whitelist_bytes)}
            </div>
            <div className="muted small" style={{ marginTop: 2, fontSize: 11 }}>
              {trafficSummary.whitelist_active ? 'Туннель БС активен для LAN устройств' : 'Серверы с меткой [БС]'}
            </div>

            {/* Квота подписки белых списков (если задан лимит у провайдера) */}
            {(() => {
              const activeWlProv = providers.find((p) => p.is_whitelist && (p.total_bytes ?? 0) > 0) || providers.find((p) => (p.total_bytes ?? 0) > 0)
              if (!activeWlProv || !activeWlProv.total_bytes || activeWlProv.total_bytes <= 0) return null
              const used = (activeWlProv.upload_bytes || 0) + (activeWlProv.download_bytes || 0)
              const total = activeWlProv.total_bytes
              const remaining = Math.max(0, total - used)
              const percent = Math.min(100, Math.round((used / total) * 100))
              const expireStr = activeWlProv.expire_timestamp
                ? new Date(activeWlProv.expire_timestamp * 1000).toLocaleDateString()
                : null

              return (
                <div style={{ marginTop: 8, paddingTop: 6, borderTop: '1px solid rgba(168, 85, 247, 0.2)', fontSize: 11 }}>
                  <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 4, flexWrap: 'wrap', gap: 4 }}>
                    <span style={{ color: '#c084fc', fontWeight: 600 }}>
                      📊 Лимит подписки: {fmtTrafficBytes(used)} / {fmtTrafficBytes(total)} ({percent}%)
                    </span>
                    <span style={{ color: remaining === 0 ? '#ef4444' : '#4ade80', fontWeight: 600 }}>
                      Осталось: {fmtTrafficBytes(remaining)}
                    </span>
                  </div>
                  <div style={{ width: '100%', height: 4, background: 'rgba(255,255,255,0.08)', borderRadius: 2, overflow: 'hidden' }}>
                    <div
                      style={{
                        width: `${percent}%`,
                        height: '100%',
                        background: percent > 90 ? '#ef4444' : percent > 75 ? '#fbbf24' : '#c084fc',
                        borderRadius: 2,
                      }}
                    />
                  </div>
                  {expireStr && (
                    <div className="muted" style={{ fontSize: 10, marginTop: 3 }}>
                      Действует до: {expireStr}
                    </div>
                  )}
                </div>
              )
            })()}
          </div>
        </div>
      </section>

      <div className="settings-columns">
        {/* ЛЕВАЯ КОЛОНКА */}
        <div className="settings-col">
          {/* FAILOVER КАРТОЧКА */}
          <section className="card" style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                <h2 style={{ margin: 0, fontSize: 16 }}>⚡ Резервирование (Failover)</h2>
                {autoSaveStatus === 'saving' && (
                  <span className="badge" style={{ borderColor: 'rgba(0, 211, 242, 0.4)', color: '#00D3F2', fontSize: 11 }}>
                    сохранение…
                  </span>
                )}
              </div>
              <span className={`badge ${settings.failover.enabled ? 'badge-online' : ''}`}>
                {settings.failover.enabled ? '🟢 включён' : '⚪ выключен'}
              </span>
            </div>

            <div className="row" style={{ justifyContent: 'space-between', alignItems: 'center' }}>
              <b>Включить автоматический failover</b>
              <div style={{ display: 'inline-flex', alignItems: 'center', gap: 10 }}>
                <label className="switch" title={settings.failover.enabled ? 'Выключить failover' : 'Включить failover'}>
                  <input
                    type="checkbox"
                    checked={settings.failover.enabled}
                    onChange={(e) => toggleFailoverEnabled(e.target.checked)}
                  />
                  <span className="slider" />
                </label>
                <span style={{ fontSize: 13, minWidth: 64, color: settings.failover.enabled ? 'var(--green)' : 'var(--muted)' }}>
                  {settings.failover.enabled ? 'Включён' : 'Выключен'}
                </span>
              </div>
            </div>

            {/* Слоты пресетов Failover */}
            <div style={{ background: 'rgba(255, 255, 255, 0.02)', border: '1px solid var(--border)', borderRadius: 10, padding: '10px 12px', display: 'flex', flexDirection: 'column', gap: 8 }}>
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                <span style={{ fontSize: 12, fontWeight: 600, color: 'var(--text)' }}>
                  📁 Слоты пресетов цепочек:
                </span>
                <span className="muted small">Быстрое переключение без ручной сборки</span>
              </div>

              <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', alignItems: 'center' }}>
                {activePresets.map((slot) => {
                  const isSelected = selectedPresetId === slot.id
                  return (
                    <div
                      key={slot.id}
                      style={{
                        display: 'inline-flex',
                        alignItems: 'center',
                        borderRadius: 20,
                        background: isSelected ? 'rgba(56, 189, 248, 0.2)' : 'rgba(255, 255, 255, 0.05)',
                        border: isSelected ? '1px solid #38bdf8' : '1px solid var(--border)',
                        padding: '3px 8px 3px 10px',
                        gap: 6,
                      }}
                    >
                      <button
                        type="button"
                        onClick={() => applyPresetSlot(slot)}
                        style={{
                          background: 'none',
                          border: 'none',
                          color: isSelected ? '#38bdf8' : 'var(--text)',
                          cursor: 'pointer',
                          fontSize: 12,
                          fontWeight: 500,
                          padding: 0,
                        }}
                        title={`Применить эту цепочку (${slot.chain?.length || 0} узлов)`}
                      >
                        {slot.name} <span style={{ opacity: 0.6, fontSize: 11 }}>({slot.chain?.length || 0})</span>
                      </button>
                      <button
                        type="button"
                        onClick={() => saveCurrentToPreset(slot.id)}
                        style={{
                          background: 'rgba(56, 189, 248, 0.25)',
                          border: 'none',
                          borderRadius: 4,
                          color: '#38bdf8',
                          cursor: 'pointer',
                          fontSize: 10.5,
                          padding: '1px 5px',
                        }}
                        title="Записать текущую настроенную цепочку в этот слот"
                      >
                        💾 Сохранить
                      </button>
                      {!['preset_default', 'preset_gaming', 'preset_streaming'].includes(slot.id) && (
                        <button
                          type="button"
                          onClick={() => deletePresetSlot(slot.id)}
                          style={{
                            background: 'none',
                            border: 'none',
                            color: '#ef4444',
                            cursor: 'pointer',
                            fontSize: 11,
                            padding: '0 2px',
                          }}
                          title="Удалить слот"
                        >
                          ✕
                        </button>
                      )}
                    </div>
                  )
                })}

                {!showNewPresetInput ? (
                  <button
                    type="button"
                    className="btn sm ghost"
                    onClick={() => setShowNewPresetInput(true)}
                    style={{ fontSize: 11, padding: '2px 8px', borderRadius: 20 }}
                  >
                    ➕ Создать слот
                  </button>
                ) : (
                  <div style={{ display: 'inline-flex', gap: 4, alignItems: 'center' }}>
                    <input
                      className="input sm"
                      placeholder="Имя слота…"
                      value={newPresetName}
                      onChange={(e) => setNewPresetName(e.target.value)}
                      style={{ width: 110, fontSize: 11, padding: '2px 6px' }}
                      onKeyDown={(e) => {
                        if (e.key === 'Enter') createNewPresetSlot()
                      }}
                    />
                    <button type="button" className="btn sm primary" onClick={createNewPresetSlot} style={{ fontSize: 11, padding: '2px 6px' }}>
                      ✓
                    </button>
                    <button type="button" className="btn sm ghost" onClick={() => setShowNewPresetInput(false)} style={{ fontSize: 11, padding: '2px 6px' }}>
                      ✕
                    </button>
                  </div>
                )}
              </div>
            </div>

            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(120px, 1fr))', gap: 10 }}>
              <label className="row" style={{ margin: 0, flexDirection: 'column', alignItems: 'flex-start', gap: 4 }}>
                <span style={{ fontSize: 12 }}>Порог пинга для смены узла, мс</span>
                <NumberInput
                  min={50}
                  max={5000}
                  step={50}
                  fallback={300}
                  value={settings.failover.ping_threshold_ms}
                  onChange={(val) => patch((s) => (s.failover.ping_threshold_ms = val))}
                />
              </label>

              <label className="row" style={{ margin: 0, flexDirection: 'column', alignItems: 'flex-start', gap: 4 }}>
                <span style={{ fontSize: 12 }}>Интервал проверки пинга, сек</span>
                <NumberInput
                  min={15}
                  max={3600}
                  step={5}
                  fallback={60}
                  value={settings.failover.interval_secs}
                  onChange={(val) => patch((s) => (s.failover.interval_secs = val))}
                />
              </label>

              <label className="row" style={{ margin: 0, flexDirection: 'column', alignItems: 'flex-start', gap: 4 }}>
                <span style={{ fontSize: 12 }} title="Длительность проверочного теста стабильности перед возвратом трафика на основной узел">
                  Тест возврата, сек
                </span>
                <NumberInput
                  min={15}
                  max={600}
                  step={15}
                  fallback={180}
                  value={settings.failover.canary_duration_secs || 180}
                  onChange={(val) => patch((s) => (s.failover.canary_duration_secs = val))}
                />
              </label>
            </div>

            <div className="row" style={{ flexDirection: 'column', alignItems: 'stretch', gap: 6 }}>
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                <span style={{ fontWeight: 600, fontSize: 13 }}>Цепочка приоритетов серверов и пулов</span>
                <span className="muted small">{chain.length} узлов в цепочке</span>
              </div>

              {/* ДВЕ РАЗДЕЛЬНЫЕ КАРТОЧКИ: 1. Активный сервер, 2. Карантин серверов (зомби) */}
              <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(280px, 1fr))', gap: 10, marginBottom: 8 }}>
                {/* КАРТОЧКА 1: Активный сервер */}
                <div
                  style={{
                    background: 'rgba(255, 255, 255, 0.02)',
                    border: '1px solid var(--border)',
                    borderLeft: '4px solid #22c55e',
                    borderRadius: 8,
                    padding: '10px 12px',
                    display: 'flex',
                    flexDirection: 'column',
                    gap: 6,
                  }}
                >
                  <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 6, minWidth: 0 }}>
                    <div style={{ display: 'flex', alignItems: 'center', gap: 5, minWidth: 0, overflow: 'hidden' }}>
                      <span className="pulse-live-dot" style={{ flexShrink: 0 }} title="Мониторинг Failover активен" />
                      <span style={{ fontSize: 12, fontWeight: 700, color: 'var(--text-bright)', whiteSpace: 'nowrap' }}>
                        Активный сервер
                      </span>
                      {status?.active_server ? (
                        <span className="badge badge-online" style={{ fontSize: 9.5, padding: '1px 5px', whiteSpace: 'nowrap', lineHeight: 1.2, flexShrink: 0 }}>
                          в сети
                        </span>
                      ) : (
                        <span className="badge" style={{ fontSize: 9.5, padding: '1px 5px', color: 'var(--muted)', whiteSpace: 'nowrap', lineHeight: 1.2, flexShrink: 0 }}>
                          не выбран
                        </span>
                      )}
                    </div>
                    <div style={{ display: 'flex', alignItems: 'center', gap: 4, flexShrink: 0 }}>
                      {failoverLastPolled && (
                        <span className="muted" style={{ fontSize: 10, whiteSpace: 'nowrap' }}>
                          {failoverLastPolled}
                        </span>
                      )}
                      <button
                        type="button"
                        className="btn sm ghost"
                        style={{
                          height: 18,
                          width: 18,
                          padding: 0,
                          display: 'inline-flex',
                          alignItems: 'center',
                          justifyContent: 'center',
                          fontSize: 10,
                          borderRadius: 4,
                        }}
                        onClick={() => {
                          refresh?.()
                          notify('Запрос свежего статуса Failover...')
                        }}
                        title="Обновить статус прямо сейчас"
                      >
                        🔄
                      </button>
                    </div>
                  </div>

                  <div style={{ display: 'flex', alignItems: 'baseline', justifyContent: 'space-between', gap: 6, minWidth: 0 }}>
                    <div
                      style={{
                        fontSize: 13,
                        fontWeight: 700,
                        color: '#4ade80',
                        overflow: 'hidden',
                        textOverflow: 'ellipsis',
                        whiteSpace: 'nowrap',
                        minWidth: 0,
                      }}
                      title={status?.active_server?.name || status?.active_server?.id || ''}
                    >
                      {cleanServerDisplayName(status?.active_server?.name || status?.active_server?.id || 'Сервер не выбран')}
                    </div>
                    {status?.active_server && status.active_server.ping_ms > 0 && (
                      <span className={'ping ' + pingClass(status.active_server.ping_ms)} style={{ fontSize: 10.5, fontWeight: 700, whiteSpace: 'nowrap', flexShrink: 0 }}>
                        {status.active_server.ping_ms} мс
                      </span>
                    )}
                  </div>

                  {/* Роль в цепочке */}
                  {status?.failover && (
                    <div style={{ display: 'flex', alignItems: 'center', gap: 6, flexWrap: 'wrap' }}>
                      <span className="muted" style={{ fontSize: 11 }}>Позиция:</span>
                      <span
                        className="badge"
                        style={{
                          fontSize: 10,
                          padding: '1px 6px',
                          background:
                            status.failover.priority_server === status.active_server?.id || status.failover.priority_chain?.[0] === status.active_server?.id
                              ? 'rgba(56, 189, 248, 0.15)'
                              : 'rgba(255, 255, 255, 0.06)',
                          color:
                            status.failover.priority_server === status.active_server?.id || status.failover.priority_chain?.[0] === status.active_server?.id
                              ? '#38bdf8'
                              : 'var(--muted)',
                        }}
                      >
                        {status.failover.priority_server === status.active_server?.id || status.failover.priority_chain?.[0] === status.active_server?.id
                          ? '👑 Основной сервер'
                          : status.failover.priority_chain?.includes(status.active_server?.id || '')
                          ? `⚡ Резерв #${status.failover.priority_chain.indexOf(status.active_server?.id || '') + 1}`
                          : '🌐 Ручной выбор'}
                      </span>
                    </div>
                  )}

                  {canaryInfo && (
                    <div style={{ fontSize: 10.5, color: '#c084fc', background: 'rgba(168, 85, 247, 0.1)', padding: '2px 5px', borderRadius: 4, border: '1px solid rgba(168, 85, 247, 0.25)', whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>
                      🧪 Тест <b>{cleanServerDisplayName(canaryInfo.target_name)}</b> ({canaryInfo.elapsed_secs}/{canaryInfo.duration_secs || 180}с)
                    </div>
                  )}
                </div>

                {/* КАРТОЧКА 2: Карантин серверов (зомби) */}
                {(() => {
                  const qList = (status?.failover?.quarantine || []).filter((q) => q.remaining_secs > 0)
                  const hasQuarantine = qList.length > 0

                  return (
                    <div
                      style={{
                        background: hasQuarantine ? 'rgba(239, 68, 68, 0.05)' : 'rgba(255, 255, 255, 0.02)',
                        border: hasQuarantine ? '1px solid rgba(239, 68, 68, 0.35)' : '1px solid var(--border)',
                        borderLeft: hasQuarantine ? '4px solid #ef4444' : '4px solid #38bdf8',
                        borderRadius: 8,
                        padding: '10px 12px',
                        display: 'flex',
                        flexDirection: 'column',
                        gap: 6,
                      }}
                    >
                      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 6 }}>
                        <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                          <span style={{ fontSize: 13 }}>{hasQuarantine ? '🧟' : '🛡️'}</span>
                          <span style={{ fontSize: 12, fontWeight: 700, color: 'var(--text-bright)' }}>
                            Карантин серверов (зомби)
                          </span>
                        </div>
                        {hasQuarantine && (
                          <button
                            type="button"
                            className="btn sm"
                            style={{
                              padding: '2px 8px',
                              fontSize: 10.5,
                              height: 22,
                              background: 'rgba(239, 68, 68, 0.18)',
                              color: '#f87171',
                              border: '1px solid rgba(239, 68, 68, 0.4)',
                              cursor: 'pointer',
                              fontWeight: 600,
                            }}
                            onClick={async () => {
                              try {
                                await apiPost('failover/quarantine/clear', {})
                                notify('Карантин всех серверов сброшен')
                                refresh?.()
                              } catch (e: any) {
                                notify(e.message || 'Ошибка снятия карантина', true)
                              }
                            }}
                            title="Мгновенно амнистировать все серверы и вернуть их в цепочку Failover"
                          >
                            🛡️ Снять весь карантин
                          </button>
                        )}
                      </div>

                      <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                        {hasQuarantine ? (
                          <span className="badge" style={{ background: 'rgba(239, 68, 68, 0.2)', color: '#f87171', border: '1px solid rgba(239, 68, 68, 0.4)', fontSize: 10.5, fontWeight: 700 }}>
                            ⚠️ В карантине: {qList.length} {qList.length === 1 ? 'узел' : 'узла(ов)'}
                          </span>
                        ) : (
                          <span className="badge badge-online" style={{ fontSize: 10.5, padding: '2px 8px' }}>
                            🟢 Все узлы в строю
                          </span>
                        )}
                      </div>

                      {hasQuarantine ? (
                        <div style={{ display: 'flex', flexDirection: 'column', gap: 4, maxHeight: 90, overflowY: 'auto', marginTop: 2 }}>
                          {qList.map((q) => {
                            const sv = servers.find((s) => s.id === q.server_id)
                            const minutesLeft = Math.ceil(q.remaining_secs / 60)
                            return (
                              <div
                                key={q.server_id}
                                style={{
                                  display: 'flex',
                                  alignItems: 'center',
                                  justifyContent: 'space-between',
                                  gap: 6,
                                  fontSize: 11,
                                  padding: '2px 6px',
                                  background: 'rgba(0, 0, 0, 0.25)',
                                  borderRadius: 4,
                                }}
                              >
                                <span style={{ color: '#fca5a5', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                                  {cleanServerDisplayName(sv?.name || q.server_id)}
                                </span>
                                <span className="muted" style={{ fontSize: 10, whiteSpace: 'nowrap' }}>
                                  {q.reason || 'таймаут'} ({minutesLeft} мин)
                                </span>
                              </div>
                            )
                          })}
                        </div>
                      ) : (
                        <div className="muted small" style={{ fontSize: 11 }}>
                          Защита от флэппинга активна. Зомби-узлы с L7-сбоями изолируются автоматически.
                        </div>
                      )}
                    </div>
                  )
                })()}
              </div>

              {/* Список узлов цепочки в виде современных карточек с акцентным бордером */}
              <div className="modal-list" style={{ maxHeight: 380, display: 'flex', flexDirection: 'column', gap: 8, overflowY: 'auto' }}>
                {chain.length === 0 && (
                  <p className="muted small" style={{ margin: '8px 0' }}>
                    Цепочка не задана — failover автоматически выбирает лучший доступный сервер.
                  </p>
                )}
                {chain.map((id, i) => {
                  const sv = servers.find((x) => x.id === id)
                  const livePing = (status?.failover?.priority_chain_pings && status.failover.priority_chain_pings[id] !== undefined)
                    ? status.failover.priority_chain_pings[id]
                    : (status?.active_server && status.active_server.id === id ? status.active_server.ping_ms : (sv?.ping_ms ?? 0))

                  const isActive = Boolean(status?.active_server && status.active_server.id === id)
                  const isCanary = Boolean(canaryInfo?.active && canaryInfo.target_id === id)
                  const liveCanarySecs = isCanary ? (canaryCountdown ?? canaryInfo?.remaining_secs ?? 0) : 0
                  const quarantineInfo = status?.failover?.quarantine?.find((q) => q.server_id === id)
                  const isZombie = Boolean(
                    status?.failover?.zombie_servers?.includes(id) ||
                    quarantineInfo?.reason?.toLowerCase().includes('зомби') ||
                    quarantineInfo?.reason?.toLowerCase().includes('zombie') ||
                    quarantineInfo?.reason?.toLowerCase().includes('l7')
                  )
                  const isQuarantined = Boolean(quarantineInfo && quarantineInfo.remaining_secs > 0)
                  const isTimeout = livePing <= 0 || livePing >= 9999

                  // Определение цвета акцентного левого бордюра и фона карточки
                  const accentColor = isActive
                    ? '#22c55e'
                    : isZombie
                    ? '#ef4444'
                    : isQuarantined
                    ? '#f97316'
                    : i === 0
                    ? '#38bdf8'
                    : 'rgba(255, 255, 255, 0.15)'

                  const bgTint = isActive
                    ? 'rgba(34, 197, 94, 0.05)'
                    : isZombie
                    ? 'rgba(239, 68, 68, 0.04)'
                    : i === 0
                    ? 'rgba(56, 189, 248, 0.03)'
                    : 'rgba(255, 255, 255, 0.015)'

                  return (
                    <div
                      key={id}
                      className="check-row priority-chain-card"
                      style={{
                        display: 'flex',
                        flexDirection: 'column',
                        gap: 6,
                        background: bgTint,
                        border: '1px solid var(--border)',
                        borderLeft: `4px solid ${accentColor}`,
                        borderRadius: 8,
                        padding: '8px 12px',
                        minWidth: 0,
                      }}
                    >
                      {/* УРОВЕНЬ 1: Номер приоритета, Чистое имя --- Статус задержки */}
                      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 10, flexWrap: 'nowrap' }}>
                        <div style={{ display: 'flex', alignItems: 'center', gap: 8, minWidth: 0, flex: 1, overflow: 'hidden' }}>
                          <span
                            style={{
                              fontSize: 10.5,
                              fontWeight: 700,
                              padding: '2px 7px',
                              borderRadius: 5,
                              background: i === 0 ? 'rgba(56, 189, 248, 0.2)' : 'rgba(255, 255, 255, 0.07)',
                              color: i === 0 ? '#38bdf8' : 'var(--muted)',
                              whiteSpace: 'nowrap',
                              flexShrink: 0,
                            }}
                          >
                            {i === 0 ? '👑 #1 Основной' : `⚡ #${i + 1} Резерв`}
                          </span>

                          <span
                            className="server-name"
                            style={{
                              fontWeight: 600,
                              fontSize: 13,
                              color: isActive ? '#4ade80' : isZombie ? '#f87171' : 'var(--text-bright)',
                              overflow: 'hidden',
                              textOverflow: 'ellipsis',
                              whiteSpace: 'nowrap',
                              minWidth: 80,
                            }}
                            title={sv ? `${cleanServerDisplayName(sv.name)} (${id})` : id}
                          >
                            {cleanServerDisplayName(sv ? sv.name : `${id} (недоступен)`)}
                          </span>
                        </div>

                        {/* Задержка / статус сети */}
                        <div style={{ display: 'flex', alignItems: 'center', gap: 6, flexShrink: 0 }}>
                          {livePing > (settings.failover.ping_threshold_ms || 300) && !isTimeout && (
                            <span
                              style={{
                                fontSize: 10,
                                padding: '1px 5px',
                                borderRadius: 4,
                                background: 'rgba(249, 115, 22, 0.15)',
                                color: '#fb923c',
                                border: '1px solid rgba(249, 115, 22, 0.3)',
                                fontWeight: 600,
                                whiteSpace: 'nowrap',
                              }}
                              title={`Пинг превышает порог ${settings.failover.ping_threshold_ms || 300} мс. При сбое узел будет пропущен.`}
                            >
                              ⚠️ &gt; {settings.failover.ping_threshold_ms || 300} мс
                            </span>
                          )}
                          {isTimeout ? (
                            <span
                              style={{
                                fontSize: 11,
                                fontWeight: 600,
                                padding: '2px 6px',
                                borderRadius: 4,
                                background: 'rgba(239, 68, 68, 0.15)',
                                color: '#f87171',
                                border: '1px solid rgba(239, 68, 68, 0.3)',
                                whiteSpace: 'nowrap',
                              }}
                            >
                              ❌ Таймаут
                            </span>
                          ) : (
                            <span className={'ping ' + pingClass(livePing)} style={{ fontSize: 11.5, fontWeight: 700, whiteSpace: 'nowrap' }}>
                              {livePing > 0 && livePing < 9999 ? `${livePing} мс` : '—'}
                            </span>
                          )}
                        </div>
                      </div>

                      {/* УРОВЕНЬ 2: Метаданные (Состояние, Пул/Провайдер, Протокол) --- Панель действий */}
                      <div
                        style={{
                          display: 'flex',
                          alignItems: 'center',
                          justifyContent: 'space-between',
                          gap: 8,
                          paddingTop: 5,
                          borderTop: '1px solid rgba(255, 255, 255, 0.04)',
                        }}
                      >
                        <div style={{ display: 'flex', alignItems: 'center', gap: 6, flexWrap: 'wrap', minWidth: 0, flex: 1 }}>
                          {isActive ? (
                            <span
                              style={{
                                fontSize: 10,
                                fontWeight: 700,
                                padding: '1px 6px',
                                borderRadius: 4,
                                background: 'rgba(34, 197, 94, 0.18)',
                                color: '#4ade80',
                                border: '1px solid rgba(34, 197, 94, 0.35)',
                                whiteSpace: 'nowrap',
                              }}
                            >
                              🟢 Активен сейчас
                            </span>
                          ) : isCanary ? (
                            <span
                              style={{
                                fontSize: 10,
                                fontWeight: 700,
                                padding: '1px 6px',
                                borderRadius: 4,
                                background: 'rgba(168, 85, 247, 0.2)',
                                color: '#c084fc',
                                border: '1px solid rgba(168, 85, 247, 0.4)',
                                whiteSpace: 'nowrap',
                              }}
                              title="Канареечный тест стабильности узла перед возвратом трафика. Клиенты остаются на резерве, пока проверяется отсутствие обрывов."
                            >
                              🧪 Тест ({liveCanarySecs}с)
                            </span>
                          ) : isZombie ? (
                            <span
                              style={{
                                fontSize: 10,
                                fontWeight: 700,
                                padding: '2px 7px',
                                borderRadius: 4,
                                background: 'rgba(239, 68, 68, 0.2)',
                                color: '#f87171',
                                border: '1px solid rgba(239, 68, 68, 0.45)',
                                cursor: 'help',
                                whiteSpace: 'nowrap',
                              }}
                              title="Сервер отвечает на TCP-пинг, но блокирует реальный веб-трафик (L7-сбой передачи данных). Временно исключён из цепочки на 10 мин."
                            >
                              🧟 Зомби (карантин {quarantineInfo ? Math.ceil(quarantineInfo.remaining_secs / 60) : 10} мин)
                            </span>
                          ) : isQuarantined ? (
                            <span
                              style={{
                                fontSize: 10,
                                fontWeight: 700,
                                padding: '1px 6px',
                                borderRadius: 4,
                                background: 'rgba(249, 115, 22, 0.2)',
                                color: '#fb923c',
                                border: '1px solid rgba(249, 115, 22, 0.4)',
                                whiteSpace: 'nowrap',
                              }}
                            >
                              🛡️ Карантин ({Math.ceil((quarantineInfo?.remaining_secs || 0) / 60)} мин)
                            </span>
                          ) : (
                            <span
                              style={{
                                fontSize: 10,
                                fontWeight: 500,
                                padding: '1px 6px',
                                borderRadius: 4,
                                background: 'rgba(255, 255, 255, 0.05)',
                                color: 'var(--muted)',
                                whiteSpace: 'nowrap',
                              }}
                            >
                              💤 В резерве
                            </span>
                          )}

                          {sv?.is_pool ? (
                            <span
                              style={{
                                fontSize: 10,
                                background: 'rgba(168, 85, 247, 0.2)',
                                color: '#c084fc',
                                padding: '1px 6px',
                                borderRadius: 4,
                                fontWeight: 600,
                              }}
                            >
                              🔀 ПУЛ
                            </span>
                          ) : sv?.provider ? (
                            <span className="tag-provider" style={{ fontSize: 10, padding: '1px 5px', cursor: 'default' }}>
                              📦 {sv.provider_name || sv.provider}
                            </span>
                          ) : null}

                          {sv?.protocol && (
                            <span className="badge-subtle" style={{ fontSize: 9.5, opacity: 0.8 }}>
                              {sv.protocol.toUpperCase()}
                            </span>
                          )}

                          {/* Адрес подключения (хост:порт): только если не дублирует имя сервера и не содержит пробелов */}
                          {sv?.host &&
                            sv.host !== sv.name &&
                            !sv.name.toLowerCase().includes(sv.host.toLowerCase()) &&
                            !sv.host.includes(' ') && (
                              <span
                                className="muted"
                                style={{
                                  fontSize: 10,
                                  fontFamily: 'monospace',
                                  maxWidth: 140,
                                  overflow: 'hidden',
                                  textOverflow: 'ellipsis',
                                  whiteSpace: 'nowrap',
                                }}
                                title={`Хост: ${sv.host}:${sv.port || 443}`}
                              >
                                {sv.host}:{sv.port || 443}
                              </span>
                            )}
                        </div>

                        {/* Панель кнопок действий */}
                        <div style={{ display: 'flex', alignItems: 'center', gap: 4, flexShrink: 0 }}>
                          {(isZombie || isQuarantined) && (
                            <button
                              type="button"
                              className="btn sm ghost"
                              style={{
                                height: 22,
                                padding: '0 7px',
                                fontSize: 10,
                                lineHeight: '20px',
                                borderRadius: 5,
                                color: '#f87171',
                                border: '1px solid rgba(239, 68, 68, 0.35)',
                                background: 'rgba(239, 68, 68, 0.1)',
                                cursor: 'pointer',
                                fontWeight: 600,
                                whiteSpace: 'nowrap',
                              }}
                              title="Досрочно снять карантин с узла"
                              onClick={async () => {
                                try {
                                  await apiPost('failover/quarantine/clear', { server_id: id })
                                  notify(`Карантин снят с ${cleanServerDisplayName(sv?.name || id)}`)
                                  refresh?.()
                                } catch (e: any) {
                                  notify(e.message || 'Ошибка снятия карантина', true)
                                }
                              }}
                            >
                              🛡️ Снять карантин
                            </button>
                          )}

                          {!isActive && (
                            <button
                              type="button"
                              className="btn sm"
                              style={{
                                height: 22,
                                padding: '0 8px',
                                fontSize: 10.5,
                                lineHeight: '20px',
                                borderRadius: 5,
                                background: 'rgba(56, 189, 248, 0.12)',
                                border: '1px solid rgba(56, 189, 248, 0.35)',
                                color: '#38bdf8',
                                cursor: 'pointer',
                                fontWeight: 600,
                              }}
                              title="Принудительно переключиться на этот сервер"
                              onClick={async () => {
                                try {
                                  if (isZombie || isQuarantined) {
                                    await apiPost('failover/quarantine/clear', { server_id: id })
                                  }
                                  await apiPost('servers/switch', { id })
                                  notify(`Переключение на ${cleanServerDisplayName(sv?.name || id)}...`)
                                  refresh?.()
                                } catch (e: any) {
                                  notify(e.message || 'Ошибка переключения', true)
                                }
                              }}
                            >
                              ⚡ Включить
                            </button>
                          )}

                          <button
                            type="button"
                            className="btn sm ghost"
                            style={{ height: 22, width: 22, padding: 0, fontSize: 10, display: 'inline-flex', alignItems: 'center', justifyContent: 'center' }}
                            disabled={i === 0}
                            onClick={() => chainMove(i, -1)}
                            title="Повысить приоритет"
                          >
                            ▲
                          </button>
                          <button
                            type="button"
                            className="btn sm ghost"
                            style={{ height: 22, width: 22, padding: 0, fontSize: 10, display: 'inline-flex', alignItems: 'center', justifyContent: 'center' }}
                            disabled={i === chain.length - 1}
                            onClick={() => chainMove(i, 1)}
                            title="Понизить приоритет"
                          >
                            ▼
                          </button>
                          <button
                            type="button"
                            className="btn sm ghost"
                            style={{ height: 22, width: 22, padding: 0, color: '#ef4444', fontSize: 11, display: 'inline-flex', alignItems: 'center', justifyContent: 'center' }}
                            onClick={() => patch((s) => (s.failover.priority_chain = chain.filter((x) => x !== id)))}
                            title="Удалить из цепочки"
                          >
                            ✕
                          </button>
                        </div>
                      </div>
                    </div>
                  )
                })}
              </div>

              {/* Аккуратный футер в одну строку: выбор добавления сервера + кнопка очистки */}
              <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 10, marginTop: 4 }}>
                <select
                  className="select"
                  value=""
                  onChange={(e) => {
                    if (!e.target.value) return
                    if (!chain.includes(e.target.value)) {
                      patch((s) => (s.failover.priority_chain = [...chain, e.target.value]))
                    }
                  }}
                  style={{ flex: 1, minWidth: 0, margin: 0 }}
                >
                  <option value="">+ добавить сервер или пул в цепочку…</option>
                  {servers.some((s) => s.is_pool && !chain.includes(s.id)) && (
                    <optgroup label="🔀 Прокси-пулы и селекторы">
                      {servers
                        .filter((s) => s.is_pool && !chain.includes(s.id))
                        .map((s) => (
                          <option key={s.id} value={s.id}>
                            🔀 {s.name} ({s.protocol || 'pool'})
                          </option>
                        ))}
                    </optgroup>
                  )}
                  <optgroup label="🌐 Узлы и серверы">
                    {servers
                      .filter((s) => !s.is_pool && !chain.includes(s.id))
                      .map((s) => {
                        const optPing = (status?.failover?.priority_chain_pings && status.failover.priority_chain_pings[s.id] !== undefined)
                          ? status.failover.priority_chain_pings[s.id]
                          : s.ping_ms
                        const isOptZombie = Boolean(
                          status?.failover?.zombie_servers?.includes(s.id) ||
                          status?.failover?.quarantine?.some((q) => q.server_id === s.id && (
                            q.reason.toLowerCase().includes('зомби') ||
                            q.reason.toLowerCase().includes('zombie') ||
                            q.reason.toLowerCase().includes('l7')
                          ))
                        )
                        return (
                          <option key={s.id} value={s.id}>
                            {s.provider_name ? `[${s.provider_name}] ` : s.provider ? `[${s.provider}] ` : ''}{s.name} · {isOptZombie ? '🧟 Зомби' : optPing > 0 ? `${optPing} мс` : '—'}
                          </option>
                        )
                      })}
                  </optgroup>
                </select>

                {chain.length > 0 && (
                  <button
                    type="button"
                    className="btn sm ghost"
                    style={{ whiteSpace: 'nowrap', flexShrink: 0, color: 'var(--muted)' }}
                    onClick={() => patch((s) => (s.failover.priority_chain = []))}
                    title="Полностью очистить цепочку приоритетов"
                  >
                    🗑️ Очистить цепочку
                  </button>
                )}
              </div>
            </div>

            <div className="row" style={{ justifyContent: 'space-between', alignItems: 'center' }}>
              <span>Возвращаться на приоритетный при восстановлении</span>
              <div style={{ display: 'inline-flex', alignItems: 'center', gap: 10 }}>
                <label className="switch" title="Автовозврат при восстановлении связи">
                  <input
                    type="checkbox"
                    checked={settings.failover.auto_restore_priority}
                    onChange={(e) => patch((s) => (s.failover.auto_restore_priority = e.target.checked))}
                  />
                  <span className="slider" />
                </label>
                <span style={{ fontSize: 13, minWidth: 64, color: settings.failover.auto_restore_priority ? 'var(--green)' : 'var(--muted)' }}>
                  {settings.failover.auto_restore_priority ? 'Да' : 'Нет'}
                </span>
              </div>
            </div>

            {/* Карантин сбойных серверов (зомби) */}
            <div className="row" style={{ justifyContent: 'space-between', alignItems: 'center' }}>
              <div>
                <b style={{ display: 'block', fontSize: 13 }}>🛡️ Карантин сбойных серверов (зомби)</b>
                <span className="muted small">Автоматически изолирует нестабильные узлы. Если выключено — серверы никогда не блокируются в карантине</span>
              </div>
              <div style={{ display: 'inline-flex', alignItems: 'center', gap: 10 }}>
                <label className="switch" title="Включить или выключить автоматический карантин">
                  <input
                    type="checkbox"
                    checked={settings.failover.quarantine_enabled ?? true}
                    onChange={(e) => patch((s) => (s.failover.quarantine_enabled = e.target.checked))}
                    data-testid="settings-quarantine-toggle"
                  />
                  <span className="slider" />
                </label>
                <span style={{ fontSize: 13, minWidth: 64, color: (settings.failover.quarantine_enabled ?? true) ? 'var(--green)' : 'var(--muted)' }}>
                  {(settings.failover.quarantine_enabled ?? true) ? 'Включён' : 'Выключен'}
                </span>
              </div>
            </div>

            {/* Best-Ping Failover (Динамический выбор узла с наименьшей задержкой) */}
            <div className="row" style={{ justifyContent: 'space-between', alignItems: 'center' }}>
              <div>
                <b style={{ display: 'block', fontSize: 13 }}>🚀 Режим Best-Ping (пул до 15 узлов)</b>
                <span className="muted small">Автоматический опрос пула серверов и переключение на узел с минимальным пингом</span>
              </div>
              <div style={{ display: 'inline-flex', alignItems: 'center', gap: 10 }}>
                <label className="switch" title="Включить режим динамического выбора лучшего пинга">
                  <input
                    type="checkbox"
                    checked={Boolean(settings.failover.best_ping_enabled)}
                    onChange={(e) => patch((s) => (s.failover.best_ping_enabled = e.target.checked))}
                    data-testid="settings-best-ping-toggle"
                  />
                  <span className="slider" />
                </label>
                <span style={{ fontSize: 13, minWidth: 64, color: settings.failover.best_ping_enabled ? 'var(--green)' : 'var(--muted)' }}>
                  {settings.failover.best_ping_enabled ? 'Включён' : 'Выключен'}
                </span>
              </div>
            </div>

            {settings.failover.best_ping_enabled && (
              <div className="row-grid" style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))', gap: 10, background: 'rgba(0, 211, 242, 0.04)', padding: '10px 12px', borderRadius: 8, border: '1px solid rgba(0, 211, 242, 0.15)' }}>
                <label className="row" style={{ margin: 0, flexDirection: 'column', alignItems: 'flex-start', gap: 4 }}>
                  <span style={{ fontSize: 12 }}>Размер пула серверов (шт.)</span>
                  <NumberInput
                    min={3}
                    max={30}
                    step={1}
                    fallback={15}
                    value={settings.failover.best_ping_pool_size ?? 15}
                    onChange={(val) => patch((s) => (s.failover.best_ping_pool_size = val))}
                  />
                </label>
                <label className="row" style={{ margin: 0, flexDirection: 'column', alignItems: 'flex-start', gap: 4 }}>
                  <span style={{ fontSize: 12 }}>Гистерезис переключения (мс)</span>
                  <NumberInput
                    min={10}
                    max={200}
                    step={5}
                    fallback={40}
                    value={settings.failover.best_ping_hysteresis_ms ?? 40}
                    onChange={(val) => patch((s) => (s.failover.best_ping_hysteresis_ms = val))}
                  />
                </label>
              </div>
            )}

            <div style={{ display: 'flex', gap: 8, justifyContent: 'space-between', alignItems: 'center', marginTop: 4, flexWrap: 'wrap' }}>
              <button className="btn" onClick={testCheck}>🔍 Тестовая проверка сейчас</button>
              <div style={{ display: 'inline-flex', alignItems: 'center', gap: 6, fontSize: 13 }}>
                {autoSaveStatus === 'saving' && (
                  <span style={{ color: '#00D3F2', display: 'inline-flex', alignItems: 'center', gap: 5 }}>
                    ⏳ Сохранение…
                  </span>
                )}
                {autoSaveStatus === 'saved' && (
                  <span style={{ color: '#34d399', display: 'inline-flex', alignItems: 'center', gap: 5 }}>
                    ✓ Сохранено автоматически
                  </span>
                )}
                {autoSaveStatus === 'error' && (
                  <span style={{ color: '#f87171', display: 'inline-flex', alignItems: 'center', gap: 5 }}>
                    ⚠️ Ошибка автосохранения
                  </span>
                )}
              </div>
            </div>
          </section>

          {/* ДОМЕНЫ */}
          <section className="card" style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', flexWrap: 'wrap', gap: 8 }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                <h2 style={{ margin: 0, fontSize: 16 }}>🌐 Домены</h2>
                <span className="badge" style={{ background: 'rgba(56, 189, 248, 0.1)', color: '#38bdf8' }}>
                  {directDomains.split('\n').filter(Boolean).length} DIRECT · {forceDomains.split('\n').filter(Boolean).length} PROXY
                </span>
              </div>
              <button type="button" className="btn sm" onClick={() => setPresetCatalogOpen(true)} title="Добавить готовые списки (YouTube, Discord, AI...)">
                ✨ Каталог пресетов
              </button>
            </div>
            <p className="muted small" style={{ margin: 0 }}>
              По одному домену в строке. Вставляются в начало <code>rules:</code> (DOMAIN-SUFFIX). Сопутствующие CDN и медиа-серверы определяются автоматически.
            </p>

            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(220px, 1fr))', gap: 12, marginTop: 4 }}>
              {/* DIRECT */}
              <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                  <b style={{ fontSize: 12.5, color: '#34d399' }}>⏭ Напрямую (DIRECT)</b>
                  <span className="muted small">{directDomains.split('\n').filter(Boolean).length} шт.</span>
                </div>
                <textarea
                  className="input"
                  rows={6}
                  placeholder={'example.com\nlocal-service.net\nw3.org'}
                  value={directDomains}
                  onChange={(e) => setDirectDomains(e.target.value)}
                  style={{ width: '100%', boxSizing: 'border-box', fontFamily: 'Consolas, monospace', fontSize: 12, resize: 'vertical' }}
                />
              </div>

              {/* PROXY */}
              <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                  <b style={{ fontSize: 12.5, color: '#38bdf8' }}>🔒 Через прокси (PROXY)</b>
                  <span className="muted small">{forceDomains.split('\n').filter(Boolean).length} шт.</span>
                </div>
                <textarea
                  className="input"
                  rows={6}
                  placeholder={'openai.com\nyoutube.com\ngithub.com'}
                  value={forceDomains}
                  onChange={(e) => setForceDomains(e.target.value)}
                  style={{ width: '100%', boxSizing: 'border-box', fontFamily: 'Consolas, monospace', fontSize: 12, resize: 'vertical' }}
                />
              </div>
            </div>

            {autoCdns.length > 0 && (
              <div style={{ marginTop: 4, padding: '8px 10px', background: 'rgba(56, 189, 248, 0.08)', border: '1px solid rgba(56, 189, 248, 0.2)', borderRadius: 6, fontSize: 12 }}>
                <div
                  style={{ color: '#38bdf8', fontWeight: 600, display: 'flex', alignItems: 'center', justifyContent: 'space-between', cursor: 'pointer', userSelect: 'none' }}
                  onClick={() => setCdnsCollapsed((prev) => !prev)}
                  title={cdnsCollapsed ? 'Развернуть список найденных CDN' : 'Свернуть список CDN'}
                >
                  <span>⚡ Автоматически подключенные CDN и медиа-сервера ({autoCdns.length})</span>
                  <span style={{ fontSize: 12, opacity: 0.85, background: 'rgba(56, 189, 248, 0.15)', padding: '2px 8px', borderRadius: 4 }}>
                    {cdnsCollapsed ? '▼ Развернуть' : '▲ Свернуть'}
                  </span>
                </div>
                {!cdnsCollapsed && (
                  <div style={{ display: 'flex', gap: 4, flexWrap: 'wrap', marginTop: 8 }}>
                    {autoCdns.map((cdn) => (
                      <span key={cdn} style={{ fontSize: 11, background: 'rgba(56, 189, 248, 0.15)', color: '#38bdf8', padding: '1px 6px', borderRadius: 4, fontFamily: 'monospace' }}>
                        {cdn}
                      </span>
                    ))}
                  </div>
                )}
              </div>
            )}

            <div style={{ display: 'flex', gap: 8, marginTop: 4, flexWrap: 'wrap' }}>
              <button className="btn primary" onClick={saveDomains} disabled={savingDomains}>
                {savingDomains ? 'Применение и поиск CDN…' : '🌐 Применить домены'}
              </button>
              <button className="btn" onClick={scanCdnManual} disabled={scanningCdn || savingDomains}>
                {scanningCdn ? '🔍 Поиск CDN…' : '🔍 Сканировать CDN сейчас'}
              </button>
            </div>
            <p className="muted small" style={{ marginTop: 2, marginBottom: 0 }}>
              💡 Фоновое сканирование выполняется автоматически 1 раз в неделю (в понедельник в 05:00) или вручную кнопкой выше, чтобы не нагружать процессор роутера.
            </p>
          </section>

          {/* ОПОВЕЩЕНИЯ (TELEGRAM / WEBHOOK) */}
          <section className="card">
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 8 }}>
              <h2 style={{ margin: 0 }}>🔔 Оповещения о сбоях</h2>
              <span className={`badge ${telegramEnabled ? 'badge-online' : ''}`}>
                {telegramEnabled ? '🟢 Telegram вкл' : '⚪ выкл'}
              </span>
            </div>
            <p className="muted small">Мгновенные уведомления в Telegram при падении серверов и переключении Failover.</p>
            <div className="row" style={{ justifyContent: 'space-between', alignItems: 'center' }}>
              <b>Telegram оповещения</b>
              <label className="switch">
                <input
                  type="checkbox"
                  checked={telegramEnabled}
                  onChange={(e) => setTelegramEnabled(e.target.checked)}
                />
                <span className="slider" />
              </label>
            </div>

            {/* BOT TOKEN */}
            <label className="row" style={{ alignItems: 'flex-start', marginTop: 8 }}>
              <span style={{ display: 'inline-flex', alignItems: 'center', gap: 6, paddingTop: 6 }}>
                Bot Token
                <button
                  type="button"
                  className={`badge-help-btn ${activeNotifHelp === 'token' ? 'active' : ''}`}
                  title="Где взять токен бота? Нажмите для инструкции"
                  onClick={() => setActiveNotifHelp((prev) => (prev === 'token' ? null : 'token'))}
                >
                  ?
                </button>
              </span>
              <div style={{ flex: 1, display: 'flex', flexDirection: 'column', gap: 3 }}>
                <input
                  className="input"
                  placeholder="123456789:ABCdefGhIJKlmNoPQRstuVWXyz"
                  value={telegramBotToken}
                  onChange={(e) => setTelegramBotToken(e.target.value)}
                  style={notifTokenWarning ? { borderColor: 'var(--red, #ef4444)' } : undefined}
                />
                {notifTokenWarning && (
                  <span style={{ fontSize: 11, color: 'var(--red, #ef4444)', lineHeight: 1.3 }}>
                    ⚠️ {notifTokenWarning}
                  </span>
                )}
              </div>
            </label>
            {activeNotifHelp === 'token' && (
              <div className="help-box-tip" style={{ marginTop: 4, marginBottom: 8 }}>
                <div style={{ fontWeight: 600, marginBottom: 4, color: 'var(--accent, #38bdf8)' }}>🤖 Как получить Bot Token:</div>
                <ol style={{ margin: 0, paddingLeft: 18, fontSize: 12, lineHeight: 1.5 }}>
                  <li>Откройте официального бота <a href="https://t.me/BotFather" target="_blank" rel="noreferrer" style={{ color: 'var(--accent, #38bdf8)', textDecoration: 'underline' }}>@BotFather</a> в Telegram.</li>
                  <li>Отправьте команду <code>/newbot</code>, укажите имя и username (должен оканчиваться на <code>bot</code>).</li>
                  <li>Скопируйте выданный токен <b>целиком</b> (вид: <code>8959891087:AAHWke...</code>) и вставьте в это поле. Не удаляйте цифры в начале!</li>
                </ol>
              </div>
            )}

            {/* CHAT ID */}
            <label className="row" style={{ alignItems: 'flex-start', marginTop: 8 }}>
              <span style={{ display: 'inline-flex', alignItems: 'center', gap: 6, paddingTop: 6 }}>
                Chat ID
                <button
                  type="button"
                  className={`badge-help-btn ${activeNotifHelp === 'chatId' ? 'active' : ''}`}
                  title="Как узнать свой Chat ID? Нажмите для инструкции"
                  onClick={() => setActiveNotifHelp((prev) => (prev === 'chatId' ? null : 'chatId'))}
                >
                  ?
                </button>
              </span>
              <div style={{ flex: 1, display: 'flex', flexDirection: 'column', gap: 3 }}>
                <input
                  className="input"
                  placeholder="123456789 или -1001234567890"
                  value={telegramChatId}
                  onChange={(e) => setTelegramChatId(e.target.value)}
                  style={notifChatIdWarning ? { borderColor: 'var(--red, #ef4444)' } : undefined}
                />
                {notifChatIdWarning && (
                  <span style={{ fontSize: 11, color: 'var(--red, #ef4444)', lineHeight: 1.3 }}>
                    {notifChatIdWarning}
                  </span>
                )}
              </div>
            </label>
            {activeNotifHelp === 'chatId' && (
              <div className="help-box-tip" style={{ marginTop: 4, marginBottom: 8 }}>
                <div style={{ fontWeight: 600, marginBottom: 4, color: 'var(--accent, #38bdf8)' }}>🆔 Как узнать ваш Chat ID:</div>
                <ol style={{ margin: 0, paddingLeft: 18, fontSize: 12, lineHeight: 1.5 }}>
                  <li>Откройте бота <a href="https://t.me/userinfobot" target="_blank" rel="noreferrer" style={{ color: 'var(--accent, #38bdf8)', textDecoration: 'underline' }}>@userinfobot</a> (или @getmyid_bot) и нажмите <b>/start</b>.</li>
                  <li>Скопируйте ваш числовой <code>Id</code> (например, <code>712345678</code>) и вставьте в это поле.</li>
                  <li>⚠️ <b>Обязательно:</b> зайдите в вашего <b>созданного бота</b> и нажмите кнопку <b>СТАРТ (/start)</b>, иначе Telegram запретит боту писать вам!</li>
                  <li><i>Для группы: добавьте бота в группу и укажите ID группы (начинается с <code>-100...</code>).</i></li>
                </ol>
              </div>
            )}

            {/* WEBHOOK URL */}
            <label className="row" style={{ alignItems: 'flex-start', marginTop: 8 }}>
              <span style={{ display: 'inline-flex', alignItems: 'center', gap: 6, paddingTop: 6 }}>
                Webhook URL
                <button
                  type="button"
                  className={`badge-help-btn ${activeNotifHelp === 'webhook' ? 'active' : ''}`}
                  title="Что такое Webhook URL? Нажмите для справки"
                  onClick={() => setActiveNotifHelp((prev) => (prev === 'webhook' ? null : 'webhook'))}
                >
                  ?
                </button>
              </span>
              <div style={{ flex: 1, display: 'flex', flexDirection: 'column', gap: 3 }}>
                <input
                  className="input"
                  placeholder="https://my-server.com/api/failover-hook"
                  value={webhookUrl}
                  onChange={(e) => setWebhookUrl(e.target.value)}
                />
                <span className="muted small" style={{ fontSize: 11 }}>
                  💡 Не требуется для Telegram! Используется только для Discord, Home Assistant или внешних серверов.
                </span>
              </div>
            </label>
            {activeNotifHelp === 'webhook' && (
              <div className="help-box-tip" style={{ marginTop: 4, marginBottom: 8 }}>
                <div style={{ fontWeight: 600, marginBottom: 4, color: 'var(--accent, #38bdf8)' }}>🔗 Для чего нужно поле Webhook URL:</div>
                <p style={{ margin: 0, fontSize: 12, lineHeight: 1.5 }}>
                  Если вы настраиваете оповещения в <b>Telegram</b> — оставьте это поле <b>пустым</b>.<br />
                  Webhook URL нужен только для автоматической отправки JSON-событий в сторонние сервисы:
                </p>
                <ul style={{ margin: '4px 0 0 0', paddingLeft: 18, fontSize: 12, lineHeight: 1.5 }}>
                  <li><b>Discord</b> (URL вебхука текстового канала из настроек сервера)</li>
                  <li><b>Home Assistant / Node-RED / n8n</b> (умный дом)</li>
                  <li>Собственные сервера и скрипты мониторинга</li>
                </ul>
              </div>
            )}

            <div style={{ display: 'flex', gap: 8, marginTop: 12, flexWrap: 'wrap' }}>
              <button className="btn primary" onClick={handleSaveNotifications} disabled={notifSaving}>
                {notifSaving ? 'Сохранение…' : '💾 Сохранить оповещения'}
              </button>
              <button className="btn" onClick={handleTestNotification} disabled={notifTesting}>
                {notifTesting ? 'Отправка…' : '💬 Тестовое сообщение'}
              </button>
            </div>
          </section>

          {/* ADBLOCK */}
          <section className="card">
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 8 }}>
              <h2 style={{ margin: 0 }}>🛡️ Блокировка рекламы (AdBlock)</h2>
              <span className={`badge ${adblockEnabled ? 'badge-online' : ''}`} style={{ color: adblockEnabled ? '#22c55e' : 'var(--muted)' }}>
                {adblockEnabled ? '🟢 Активно на роутере' : '⚪ Отключено'}
              </span>
            </div>
            <p className="muted small">
              Блокирует рекламные баннеры, видеовставки, счетчики трекеров и аналитику для всех устройств в сети без установки расширений в браузеры (правило <code>category-ads-all</code> из базы GeoSite).
            </p>
            <div className="row" style={{ justifyContent: 'space-between', alignItems: 'center' }}>
              <span>Включить AdBlock для всех устройств</span>
              <div style={{ display: 'inline-flex', alignItems: 'center', gap: 10 }}>
                <label className="switch">
                  <input
                    type="checkbox"
                    checked={adblockEnabled}
                    disabled={adblockBusy}
                    onChange={(e) => handleToggleAdblock(e.target.checked)}
                  />
                  <span className="slider" />
                </label>
                <span style={{ fontSize: 13, minWidth: 64, color: adblockEnabled ? 'var(--green)' : 'var(--muted)' }}>
                  {adblockBusy ? '⏳…' : adblockEnabled ? 'Да' : 'Нет'}
                </span>
              </div>
            </div>
          </section>

          {/* RCI (KEENETIC) */}
          <section className="card">
            <h2>🔌 RCI (Keenetic)</h2>
            <label className="row"><span>Host</span>
              <input className="input" value={settings.rci.host} onChange={(e) => patch((s) => (s.rci.host = e.target.value))} />
            </label>
            <label className="row"><span>Порт</span>
              <NumberInput
                min={1}
                max={65535}
                fallback={79}
                value={settings.rci.port}
                onChange={(val) => patch((s) => (s.rci.port = val))}
              />
            </label>
            <label className="row"><span>Логин</span>
              <input className="input" value={settings.rci.login} onChange={(e) => patch((s) => (s.rci.login = e.target.value))} />
            </label>
            <label className="row"><span>Пароль (fallback)</span>
              <input className="input" type="password" value={settings.rci.password} onChange={(e) => patch((s) => (s.rci.password = e.target.value))} />
            </label>
            <label className="row"><span>Токен (X-Ndma-Tkn)</span>
              <input className="input" value={settings.rci.token} placeholder="пусто = из /opt/etc/xkeen/xkeen.json"
                onChange={(e) => patch((s) => (s.rci.token = e.target.value))} />
            </label>
            <p className="muted small">Если токен не задан, панель возьмёт его из /opt/etc/xkeen/xkeen.json; иначе — challenge-auth.</p>
          </section>

          {/* СЕРВИС XKEEN */}
          <section className="card">
            <h2>🖥 Сервис XKeen</h2>
            <p className="muted small">Restart перегенерирует config.yaml — настройки маршрутизации возвращаются к исходным (до любых изменений из панели).</p>
            <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
              <button className="btn" disabled={svcBusy !== ''} onClick={() => svc('status')}>📊 Статус</button>
              <button className="btn" disabled={svcBusy !== ''} onClick={() => svc('start')}>▶ Старт</button>
              <button className="btn" disabled={svcBusy !== ''} onClick={() => svc('restart')}>🔄 Рестарт</button>
              <button className="btn" style={{ borderColor: 'var(--red)', color: 'var(--red)' }} disabled={svcBusy !== ''} onClick={() => { if (confirm('Остановить сервис XKeen? Интернет через прокси пропадёт.')) svc('stop') }}>⏹ Стоп</button>
            </div>
            <label className="row" style={{ marginTop: 10 }}><span>Init-скрипт XKeen</span>
              <input className="input" value={settings.system?.xkeen_init ?? '/opt/etc/init.d/S05xkeen'}
                onChange={(e) => patch((s) => (s.system.xkeen_init = e.target.value))} />
            </label>
          </section>
        </div>

        {/* ПРАВАЯ КОЛОНКА */}
        <div className="settings-col">
          {/* DNS РЕЖИМ (MIHOMO) */}
          <section className="card">
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 8 }}>
              <h2 style={{ margin: 0 }}>🧭 DNS Режим (Mihomo)</h2>
              <span className="badge" style={{ textTransform: 'uppercase' }}>{dnsMode}</span>
            </div>
            <p className="muted small">Режим обработки DNS-запросов ядром. Изменение режима перезапускает службу DNS.</p>
            <div className="tile-options-grid" style={{ gridTemplateColumns: 'repeat(auto-fit, minmax(220px, 1fr))' }}>
              <label className={`option-tile-card ${dnsMode === 'fake-ip' ? 'active' : ''}`}>
                <div className="option-tile-header">
                  <input
                    type="radio"
                    name="dns_mode_setting"
                    checked={dnsMode === 'fake-ip'}
                    onChange={() => handleSetDnsMode('fake-ip')}
                    disabled={dnsModeBusy}
                  />
                  <span className="option-tile-title">⚡ Fake-IP</span>
                  <span className="badge badge-accent" style={{ fontSize: 10, padding: '1px 5px', marginLeft: 'auto' }}>
                    Рекомендуется
                  </span>
                </div>
                <div className="option-tile-desc">
                  Мгновенный отклик DNS (~1 мс), эффективный обход DPI и блокировок, идеален для стримов, мессенджеров и игр.
                </div>
              </label>

              <label className={`option-tile-card ${dnsMode === 'redir-host' ? 'active' : ''}`}>
                <div className="option-tile-header">
                  <input
                    type="radio"
                    name="dns_mode_setting"
                    checked={dnsMode === 'redir-host'}
                    onChange={() => handleSetDnsMode('redir-host')}
                    disabled={dnsModeBusy}
                  />
                  <span className="option-tile-title">🌐 Redir-Host</span>
                  <span className="badge" style={{ fontSize: 10, padding: '1px 5px', marginLeft: 'auto' }}>
                    Прямой
                  </span>
                </div>
                <div className="option-tile-desc">
                  Классический резолв реальных IP-адресов. Используйте, если требуются локальные домены роутера (.keenetic.io / Home LAN).
                </div>
              </label>
            </div>
          </section>


          {/* GEOIP / GEOSITE */}
          <section className="card">
            <h2>🔄 Базы данных GeoIP и GeoSite</h2>
            <p className="muted small">
              Используются ядром Mihomo для точного определения стран и категорий сайтов (включая списки рекламы AdBlock). Загрузка выполняется через прокси Mihomo для стабильности.
            </p>
            <div className="stats-grid four-col" style={{ marginBottom: 12 }}>
              <div className="stat-card">
                <div className="stat-label">База GeoIP</div>
                <div className="stat-value" style={{ fontSize: 16 }}>
                  {geoInfo?.geoip?.size ? `${(geoInfo.geoip.size / (1024 * 1024)).toFixed(1)} МБ` : '—'}
                </div>
                <div className="muted small">{geoInfo?.geoip?.updated_at || 'Mihomo Core'}</div>
              </div>
              <div className="stat-card">
                <div className="stat-label">База GeoSite</div>
                <div className="stat-value" style={{ fontSize: 16 }}>
                  {geoInfo?.geosite?.size ? `${(geoInfo.geosite.size / (1024 * 1024)).toFixed(1)} МБ` : '—'}
                </div>
                <div className="muted small">{geoInfo?.geosite?.updated_at || 'Meta Rules Dat'}</div>
              </div>
              <div className="stat-card">
                <div className="stat-label">Источник баз</div>
                <div className="stat-value" style={{ fontSize: 14, color: '#38bdf8' }}>
                  MetaCubeX
                </div>
                <div className="muted small">Загрузка через прокси</div>
              </div>
              <div className="stat-card">
                <div className="stat-label">Интеграция</div>
                <div className="stat-value" style={{ fontSize: 14, color: '#22c55e' }}>
                  ✓ Активны
                </div>
                <div className="muted small">Маршрутизация & AdBlock</div>
              </div>
            </div>
            <button
              type="button"
              className="btn primary"
              onClick={handleUpdateGeo}
              disabled={geoUpdating}
            >
              {geoUpdating ? '⏳ Загрузка баз (может занять до 1 мин)…' : '🔄 Обновить GeoIP / GeoSite базы'}
            </button>
          </section>

          {/* MIHOMO */}
          <section className="card">
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 10 }}>
              <h2 style={{ margin: 0 }}>⚙️ Mihomo</h2>
              <button
                type="button"
                className="btn sm primary"
                onClick={() => window.dispatchEvent(new CustomEvent('xr:open-mihomo-modal'))}
                title="Открыть список релизов MetaCubeX и обновить ядро Mihomo"
              >
                🚀 Релизы и обновление ядра
              </button>
            </div>
            <label className="row"><span>Host</span>
              <input className="input" value={settings.mihomo.host} onChange={(e) => patch((s) => (s.mihomo.host = e.target.value))} />
            </label>
            <label className="row"><span>Порт</span>
              <NumberInput
                min={1}
                max={65535}
                fallback={9090}
                value={settings.mihomo.port}
                onChange={(val) => patch((s) => (s.mihomo.port = val))}
              />
            </label>
            <label className="row"><span>Secret</span>
              <input className="input" value={settings.mihomo.secret} onChange={(e) => patch((s) => (s.mihomo.secret = e.target.value))} />
            </label>
            <label className="row"><span>Путь к config.yaml</span>
              <input className="input" value={settings.mihomo.config_path} onChange={(e) => patch((s) => (s.mihomo.config_path = e.target.value))} />
            </label>
            <label className="row"><span>Провайдеры групп устройств</span>
              <input
                className="input"
                placeholder="пусто = авто из config.yaml"
                value={(settings.mihomo.device_providers ?? []).join(', ')}
                onChange={(e) => patch((s) => (s.mihomo.device_providers = e.target.value.split(',').map((x) => x.trim()).filter(Boolean)))}
              />
            </label>
            <p className="muted small">Имена proxy-providers, подключаемые к per-device группам (use:). Пусто — берутся все из config.yaml автоматически.</p>
          </section>

          {/* БЭКАПЫ */}
          <section className="card">
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 6 }}>
              <h2 style={{ margin: 0 }}>💾 Бэкапы (.xkbak)</h2>
              <div style={{ display: 'flex', gap: 6 }}>
                <button className="btn sm primary" onClick={createBackup} disabled={backupBusy}>
                  {backupBusy ? 'Создание…' : '＋ Создать бэкап'}
                </button>
                <button className="btn sm" onClick={() => backupFileRef.current?.click()} disabled={backupBusy} title="Загрузить архив бэкапа с компьютера">
                  📤 Загрузить архив
                </button>
                <input
                  ref={backupFileRef}
                  type="file"
                  accept=".xkbak,.tar.gz,.tar,.zip"
                  style={{ display: 'none' }}
                  onChange={handleImportBackup}
                />
              </div>
            </div>
            <p className="muted small">Снимок config.yaml (Mihomo) + config.json (панель). Каталог: {backupDir || '…'}</p>
            <div className="modal-list" style={{ marginTop: 10 }}>
              {backups.length === 0 && <p className="muted small">Бэкапов пока нет.</p>}
              {backups.map((b) => (
                <div key={b} className="check-row" style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
                  <span className="server-name" style={{ flex: 1 }}>{b}</span>
                  <button className="btn sm" disabled={backupBusy} onClick={() => restoreBackup(b)}>Восстановить</button>
                  <a className="btn sm ghost" href={`/api/backups/export/${encodeURIComponent(b)}`} download title="Скачать архив бэкапа (.xkbak)">
                    ⬇ .xkbak
                  </a>
                  <button className="btn sm ghost" disabled={backupBusy} onClick={() => deleteBackup(b)}>✕</button>
                </div>
              ))}
            </div>
            <label className="row" style={{ marginTop: 10 }}><span>Каталог бэкапов</span>
              <input className="input" value={settings.system?.backup_dir ?? ''}
                onChange={(e) => patch((s) => (s.system.backup_dir = e.target.value))} />
            </label>
          </section>

          {/* ПАНЕЛЬ */}
          <section className="card">
            <h2>🖥 Панель</h2>
            <label className="row"><span>Интервал автообновления, сек</span>
              <NumberInput
                min={3}
                max={300}
                step={1}
                fallback={10}
                value={settings.refresh_interval_sec}
                onChange={(val) => patch((s) => (s.refresh_interval_sec = val))}
              />
            </label>
            <label className="row"><span>Уровень логов</span>
              <select className="select" value={settings.logs?.level ?? 'info'}
                onChange={(e) => patch((s) => { s.logs.level = e.target.value })}>
                <option value="info">info (подробно)</option>
                <option value="warn">warn (предупреждения и ошибки)</option>
                <option value="error">error (только ошибки)</option>
              </select>
            </label>
            <label className="row" style={{ justifyContent: 'flex-start', gap: 8, cursor: 'pointer' }}>
              <input type="checkbox" checked={settings.logs?.log_requests ?? true}
                onChange={(e) => patch((s) => (s.logs.log_requests = e.target.checked))} />
              <span>Логировать HTTP-запросы к панели</span>
            </label>
            <label className="row"><span>Удалённый syslog (host:port, UDP)</span>
              <input className="input" placeholder="пусто = не отправлять, напр. 192.168.2.10:514"
                value={settings.logs?.remote_syslog ?? ''}
                onChange={(e) => patch((s) => (s.logs.remote_syslog = e.target.value))} />
            </label>
            <p className="muted small">Syslog начнёт работать после перезапуска панели (restart в разделе «Сервис XKeen» не нужен — перезапуск S99xkeen-route).</p>
            <button className="btn primary" onClick={save} disabled={saving}>
              {saving ? 'Сохранение…' : '💾 Сохранить настройки'}
            </button>
            <p className="muted small">Конфиг хранится в /opt/etc/xkeen-route/config.json (путь — на дашборде).</p>

            <div style={{ marginTop: 14, borderTop: '1px solid var(--border)', paddingTop: 12 }}>
              <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 12, flexWrap: 'wrap' }}>
                <span className="muted small">
                  Версия панели: <b style={{ color: 'var(--text-bright)' }}>{status?.version || '…'}</b>
                </span>
                <button
                  type="button"
                  className="btn sm"
                  onClick={() => window.dispatchEvent(new CustomEvent('xr:open-update-modal'))}
                  title="Открыть окно управления версией и обновления панели"
                >
                  🚀 Меню обновления панели
                </button>
              </div>
            </div>
          </section>
        </div>
      </div>

      {/* БЕЗОПАСНОСТЬ И ПАРОЛЬ (Сжатая Apple-карточка) */}
      <section className="card" style={{ padding: authEnabled ? '16px' : '8px 16px', transition: 'all 0.25s ease' }}>
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', minHeight: authEnabled ? 'auto' : 36 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
            <span style={{ fontSize: 16 }}>🔐</span>
            <div>
              <b style={{ fontSize: 13.5 }}>Безопасность и пароль</b>
              {!authEnabled && (
                <span className="muted small" style={{ marginLeft: 8, fontSize: 12 }}>
                  (пароль выключен — свободный вход)
                </span>
              )}
            </div>
          </div>
          <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
            <span className={`badge ${authEnabled ? 'badge-online' : ''}`} style={{ fontSize: 11, padding: '2px 8px' }}>
              {authEnabled ? '🟢 защита включена' : '⚪ без пароля'}
            </span>
            <label className="switch" style={{ margin: 0 }}>
              <input
                type="checkbox"
                checked={authEnabled}
                onChange={async (e) => {
                  const checked = e.target.checked
                  setAuthEnabled(checked)
                  if (!checked && settings?.auth?.enabled) {
                    if (window.confirm('Отключить защиту паролем для веб-панели?')) {
                      setAuthSaving(true)
                      try {
                        await apiPost('auth/change-password', {
                          enabled: false,
                          new_password: '',
                        })
                        notify('Авторизация отключена')
                        refresh?.()
                      } catch (err: any) {
                        notify(err.message || 'Ошибка отключения пароля', true)
                        setAuthEnabled(true)
                      } finally {
                        setAuthSaving(false)
                      }
                    } else {
                      setAuthEnabled(true)
                    }
                  }
                }}
              />
              <span className="slider" />
            </label>
          </div>
        </div>

        {authEnabled && (
          <div style={{ marginTop: 12, borderTop: '1px solid rgba(255,255,255,0.06)', paddingTop: 12 }}>
            <p className="muted small" style={{ margin: '0 0 10px' }}>Защита веб-панели паролем с сессией (24 часа).</p>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
              {settings?.auth?.enabled && (
                <label className="row">
                  <span>Текущий пароль</span>
                  <input
                    className="input"
                    type="password"
                    placeholder="Текущий пароль (для подтверждения)"
                    value={currentPassword}
                    onChange={(e) => setCurrentPassword(e.target.value)}
                  />
                </label>
              )}
              <label className="row">
                <span>Новый пароль</span>
                <input
                  className="input"
                  type="password"
                  placeholder="Введите новый пароль"
                  value={newPassword}
                  onChange={(e) => setNewPassword(e.target.value)}
                />
              </label>
              <label className="row">
                <span>Повторите пароль</span>
                <input
                  className="input"
                  type="password"
                  placeholder="Повторите новый пароль"
                  value={confirmPassword}
                  onChange={(e) => setConfirmPassword(e.target.value)}
                />
              </label>
            </div>

            <button className="btn primary" style={{ marginTop: 12 }} onClick={handleSaveAuth} disabled={authSaving}>
              {authSaving ? 'Сохранение…' : '💾 Сохранить настройки доступа'}
            </button>
            <p className="muted small" style={{ marginTop: 8, marginBottom: 0 }}>
              💡 Сброс пароля при утере через SSH: <code style={{ color: 'var(--accent)' }}>xkeen-route reset-password</code>
            </p>
          </div>
        )}
      </section>

      {/* ФАЙЛ ПОДКАЧКИ (SWAP НА USB) */}
      <SwapManager notify={notify} />

      {/* ДИСПЕТЧЕР ЗАДАЧ (HTOP) */}
      <section id="task-manager-section" className="card" style={{ padding: 16 }}>
        <TaskManager notify={notify} />
      </section>

      {/* ЖУРНАЛ ЛОГОВ С ПОЛНЫМ ФУНКЦИОНАЛОМ LOGSVIEWER */}
      <section className="card" style={{ padding: 16 }}>
        <LogsViewer notify={notify} />
      </section>

      {/* МОДАЛЬНОЕ ОКНО КОНФИГ-РЕДАКТОРА */}
      <ConfigEditor
        isOpen={configEditorOpen}
        onClose={() => setConfigEditorOpen(false)}
        notify={notify}
      />

      {/* МОДАЛЬНОЕ ОКНО КАТАЛОГА ПРЕСЕТОВ */}
      <PresetCatalogModal
        isOpen={presetCatalogOpen}
        onClose={() => setPresetCatalogOpen(false)}
        onApplyPreset={handleApplyPreset}
        notify={notify}
      />
    </div>
  )
}

