import { useCallback, useEffect, useRef, useState } from 'react'
import { apiGet, apiPost, apiPut } from '../api'
import { diffSettings } from '../utils/settingsDiff'
import { useDirtyGuard } from '../utils/dirtyGuard'
import { validateAndParseBackupPayload } from '../utils/backupPayload'
import {
  type AppSettings,
  type DnsMode,
  type DnsEnhancedMode,
  type ServerInfo,
  type ProviderInfo,
  type StatusInfo,
} from '../types'
import { TrafficSummaryCard } from './settings/TrafficSummaryCard'
import { FailoverSection } from './settings/FailoverSection'
import ConfigEditor from './ConfigEditor'
import PresetCatalogModal from './PresetCatalogModal'
import LogsViewer from './LogsViewer'
import TaskManager from './TaskManager'
import {
  SwapManager,
  fmtTrafficBytes,
} from './settings/SwapManager'
import { DomainsAndNotificationsSection } from './settings/DomainsAndNotificationsSection'
import { SystemSettingsCards, type GeoInfo } from './settings/SystemSettingsCards'
import { SecurityCard } from './settings/SecurityCard'

export { SwapManager, cleanServerDisplayName, formatActiveServerStatus } from './settings/SwapManager'
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
  const [savedDirectDomains, setSavedDirectDomains] = useState('')
  const [savedForceDomains, setSavedForceDomains] = useState('')
  useDirtyGuard('settings', directDomains !== savedDirectDomains || forceDomains !== savedForceDomains)
  const [autoCdns, setAutoCdns] = useState<string[]>([])
  const [cdnsCollapsed, setCdnsCollapsed] = useState(true)
  const [savingDomains, setSavingDomains] = useState(false)
  const [scanningCdn, setScanningCdn] = useState(false)
  const [domainsRevision, setDomainsRevision] = useState<string | undefined>(undefined)

  const hasUnsavedDirect = directDomains.trim() !== savedDirectDomains.trim()
  const hasUnsavedForce = forceDomains.trim() !== savedForceDomains.trim()
  const hasUnsavedDomains = hasUnsavedDirect || hasUnsavedForce
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
        direct_bytes: res.direct_bytes !== undefined ? res.direct_bytes : (target === 'all' || target === 'direct' ? 0 : prev.direct_bytes),
        proxy_bytes: res.proxy_bytes !== undefined ? res.proxy_bytes : (target === 'all' || target === 'vps' ? 0 : prev.proxy_bytes),
        whitelist_bytes: res.whitelist_bytes !== undefined ? res.whitelist_bytes : (target === 'all' || target === 'whitelist' ? 0 : prev.whitelist_bytes),
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
        notifTokenWarning = 'Токен неполный! В начале обязательно должен быть числовой ID бота и двоеточие (например: 123456789:AAAA...)'
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
  // Снимок настроек, с которым синхронизирован бэкенд: на сохранение уходит только дифф
  const loadedSettingsRef = useRef<AppSettings | null>(null)
  const autoSaveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const savedTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  // --- Пресеты Failover слотов ---
  const [selectedPresetId, setSelectedPresetId] = useState<string | null>(null)
  const [newPresetName, setNewPresetName] = useState('')
  const [showNewPresetInput, setShowNewPresetInput] = useState(false)
  const [priorityTab, setPriorityTab] = useState<'vps' | 'bs' | 'providers'>('vps')

  // --- AdBlock ---
  const [adblockEnabled, setAdblockEnabled] = useState(false)
  const [adblockBusy, setAdblockBusy] = useState(false)

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
      loadedSettingsRef.current = JSON.parse(JSON.stringify(s))
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
    const fetchProviders = () => {
      apiGet<{ providers: ProviderInfo[] }>('providers')
        .then((d) => setProviders(d.providers || []))
        .catch(() => {})
    }
    fetchProviders()
    const provInterval = setInterval(fetchProviders, 15000)
    apiGet<{ direct: string[]; force: string[]; auto_cdns?: string[]; revision?: string }>('domains')
      .then((d) => {
        const dStr = (Array.isArray(d?.direct) ? d.direct : []).join('\n')
        const fStr = (Array.isArray(d?.force) ? d.force : []).join('\n')
        setDirectDomains(dStr)
        setForceDomains(fStr)
        setSavedDirectDomains(dStr)
        setSavedForceDomains(fStr)
        if (d.auto_cdns) setAutoCdns(d.auto_cdns)
        if (d.revision) setDomainsRevision(d.revision)
      })
      .catch((e) => notify(e instanceof Error ? e.message : 'Ошибка загрузки списков доменов', true))
    apiGet<DnsMode>('dns/mode')
      .then((d) => setDnsMode(d.enhanced_mode))
      .catch((e) => notify(e instanceof Error ? e.message : 'Ошибка загрузки режима DNS', true))
    return () => clearInterval(provInterval)
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
        const savedFailover = initialFailoverRef.current ? JSON.parse(initialFailoverRef.current) : {}
        const changed = diffSettings(savedFailover, currentFailover)
        if (changed !== undefined) {
          await apiPut('settings', { failover: changed })
        }
        initialFailoverRef.current = failoverJson
        if (loadedSettingsRef.current) loadedSettingsRef.current.failover = currentFailover
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
      if (savedTimerRef.current) {
        clearTimeout(savedTimerRef.current)
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
      // Только флаг: раньше сюда уходили устаревшие priority_chain (через settings/priority)
      // и весь объект failover, откатывая свежие изменения цепочек
      await apiPut('settings', { failover: { enabled } })
      if (loadedSettingsRef.current) loadedSettingsRef.current.failover.enabled = enabled
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
      // Только изменённые пользователем поля: полный снимок затирал фоновые изменения
      const changed = updatedSettings ? diffSettings(loadedSettingsRef.current ?? {}, updatedSettings) : undefined
      if (changed !== undefined) {
        await apiPut('settings', changed)
      }
      if (updatedSettings) {
        loadedSettingsRef.current = JSON.parse(JSON.stringify(updatedSettings))
        setSettings(updatedSettings)
      }
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

  // --- Глобальная цепочка приоритетов (VPS) ---
  const chain = settings?.failover.priority_chain ?? (settings?.failover.priority_server ? [settings.failover.priority_server] : [])

  // --- Цепочка приоритетов серверов Белых списков (БС) ---
  const bsChain = settings?.failover.bs_priority_chain ?? []

  const bsChainMove = (idx: number, dir: -1 | 1) => {
    const next = [...bsChain]
    const j = idx + dir
    if (j < 0 || j >= next.length) return
    ;[next[idx], next[j]] = [next[j], next[idx]]
    patch((s) => (s.failover.bs_priority_chain = next))
  }
  void priorityTab
  void setPriorityTab
  void bsChainMove

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

  // --- Цепочка приоритетов провайдеров (Tier 1 vs Tier 2) ---
  const providerChain: string[] = settings?.failover?.provider_chain ?? (status?.failover?.provider_chain ?? [])

  const moveProviderChain = (idx: number, dir: -1 | 1) => {
    const next = [...providerChain]
    const j = idx + dir
    if (j < 0 || j >= next.length) return
    ;[next[idx], next[j]] = [next[j], next[idx]]
    patch((s) => {
      s.failover.provider_chain = next
    })
  }

  const toggleProviderInChain = (pId: string) => {
    const exists = providerChain.includes(pId)
    const next = exists ? providerChain.filter((id) => id !== pId) : [...providerChain, pId]
    patch((s) => {
      s.failover.provider_chain = next
    })
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
      setSavedDirectDomains(directDomains)
      setSavedForceDomains(forceDomains)
      const cdnMsg = data.auto_cdns && data.auto_cdns.length > 0 ? `, CDN: +${data.auto_cdns.length}` : ''
      const ipMsg = data.overridden_ips ? `, IP: ${data.overridden_ips}` : ''
      notify(`✓ Домены успешно применены: напрямую ${data.direct}, через прокси ${data.force}${cdnMsg}${ipMsg}`)
    } catch (e) {
      // Прямое указание пользователя: автоматически убирать домен с выводом ошибки
      setDirectDomains(savedDirectDomains)
      setForceDomains(savedForceDomains)
      const rawMsg = e instanceof Error ? e.message : 'Ошибка сохранения доменов'
      notify(`❌ ${rawMsg}. Несохранённые домены автоматически убраны из списка.`, true)
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
      const payload = validateAndParseBackupPayload(text, file.name)
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
      const results = Array.isArray(res?.results) ? res.results : []
      const hasError = results.some((r) => String(r).includes('❌'))
      const summary = results.join('\n') || res?.message || 'Тестовое оповещение успешно отправлено'
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

      {/* СВОДКА СЕТЕВОГО ТРАФИКА (Вариант 3: Монолитная панель) */}
      <TrafficSummaryCard
        trafficSummary={trafficSummary}
        providers={providers}
        fmtTrafficBytes={fmtTrafficBytes}
        handleResetTraffic={handleResetTraffic}
      />

      <div className="settings-columns">
        {/* FAILOVER КАРТОЧКА */}
        <FailoverSection
          settings={settings}
          status={status}
          servers={servers}
          providers={providers}
          autoSaveStatus={autoSaveStatus}
          failoverLastPolled={failoverLastPolled}
          canaryInfo={canaryInfo}
          canaryCountdown={canaryCountdown}
          chain={chain}
          providerChain={providerChain}
          activePresets={activePresets}
          selectedPresetId={selectedPresetId}
          showNewPresetInput={showNewPresetInput}
          setShowNewPresetInput={setShowNewPresetInput}
          newPresetName={newPresetName}
          setNewPresetName={setNewPresetName}
          patch={patch}
          toggleFailoverEnabled={toggleFailoverEnabled}
          testCheck={testCheck}
          chainMove={chainMove}
          toggleProviderInChain={toggleProviderInChain}
          moveProviderChain={moveProviderChain}
          applyPresetSlot={applyPresetSlot}
          saveCurrentToPreset={saveCurrentToPreset}
          createNewPresetSlot={createNewPresetSlot}
          deletePresetSlot={deletePresetSlot}
          notify={notify}
          refresh={refresh}
        />
          <DomainsAndNotificationsSection
            notify={notify}
            directDomains={directDomains}
            setDirectDomains={setDirectDomains}
            forceDomains={forceDomains}
            setForceDomains={setForceDomains}
            savedDirectDomains={savedDirectDomains}
            savedForceDomains={savedForceDomains}
            autoCdns={autoCdns}
            cdnsCollapsed={cdnsCollapsed}
            setCdnsCollapsed={setCdnsCollapsed}
            savingDomains={savingDomains}
            scanningCdn={scanningCdn}
            hasUnsavedDirect={hasUnsavedDirect}
            hasUnsavedForce={hasUnsavedForce}
            hasUnsavedDomains={hasUnsavedDomains}
            setPresetCatalogOpen={setPresetCatalogOpen}
            saveDomains={saveDomains}
            scanCdnManual={scanCdnManual}
            telegramEnabled={telegramEnabled}
            setTelegramEnabled={setTelegramEnabled}
            telegramBotToken={telegramBotToken}
            setTelegramBotToken={setTelegramBotToken}
            telegramChatId={telegramChatId}
            setTelegramChatId={setTelegramChatId}
            webhookUrl={webhookUrl}
            setWebhookUrl={setWebhookUrl}
            notifTesting={notifTesting}
            notifSaving={notifSaving}
            activeNotifHelp={activeNotifHelp}
            setActiveNotifHelp={setActiveNotifHelp}
            notifTokenWarning={notifTokenWarning}
            notifChatIdWarning={notifChatIdWarning}
            handleSaveNotifications={handleSaveNotifications}
            handleTestNotification={handleTestNotification}
          />

          <SystemSettingsCards
            settings={settings}
            status={status}
            saving={saving}
            patch={patch}
            save={save}
            adblockEnabled={adblockEnabled}
            adblockBusy={adblockBusy}
            handleToggleAdblock={handleToggleAdblock}
            svc={svc}
            svcBusy={svcBusy}
            dnsMode={dnsMode}
            dnsModeBusy={dnsModeBusy}
            handleSetDnsMode={handleSetDnsMode}
            geoInfo={geoInfo}
            geoUpdating={geoUpdating}
            handleUpdateGeo={handleUpdateGeo}
            backups={backups}
            backupDir={backupDir}
            backupBusy={backupBusy}
            backupFileRef={backupFileRef}
            createBackup={createBackup}
            handleImportBackup={handleImportBackup}
            restoreBackup={restoreBackup}
            deleteBackup={deleteBackup}
          />
      </div>

      <SecurityCard
        settings={settings}
        authEnabled={authEnabled}
        setAuthEnabled={setAuthEnabled}
        currentPassword={currentPassword}
        setCurrentPassword={setCurrentPassword}
        newPassword={newPassword}
        setNewPassword={setNewPassword}
        confirmPassword={confirmPassword}
        setConfirmPassword={setConfirmPassword}
        authSaving={authSaving}
        setAuthSaving={setAuthSaving}
        handleSaveAuth={handleSaveAuth}
        notify={notify}
        refresh={refresh}
      />

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

