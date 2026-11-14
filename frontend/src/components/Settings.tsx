import { useCallback, useEffect, useRef, useState } from 'react'
import { apiGet, apiPost, apiPut } from '../api'
import { diffSettings } from '../utils/settingsDiff'
import { useDirtyGuard } from '../utils/dirtyGuard'
import {
  pingClass,
  type AppSettings,
  type DnsMode,
  type DnsEnhancedMode,
  type ServerInfo,
  type ProviderInfo,
  type StatusInfo,
} from '../types'
import NumberInput from './NumberInput'
import { formatQuotaBytes } from './Servers'
import ConfigEditor from './ConfigEditor'
import PresetCatalogModal from './PresetCatalogModal'
import LogsViewer from './LogsViewer'
import TaskManager from './TaskManager'
import {
  SwapManager,
  fmtTrafficBytes,
  cleanServerDisplayName,
} from './settings/SwapManager'
import { WhitelistDetectorSection } from './settings/WhitelistDetectorSection'
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
        const dStr = d.direct.join('\n')
        const fStr = d.force.join('\n')
        setDirectDomains(dStr)
        setForceDomains(fStr)
        setSavedDirectDomains(dStr)
        setSavedForceDomains(fStr)
        if (d.auto_cdns) setAutoCdns(d.auto_cdns)
        if (d.revision) setDomainsRevision(d.revision)
      })
      .catch(() => {})
    apiGet<DnsMode>('dns/mode')
      .then((d) => setDnsMode(d.enhanced_mode))
      .catch(() => {})
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

      {/* СВОДКА СЕТЕВОГО ТРАФИКА (Вариант 3: Монолитная панель) */}
      <section className="card" style={{ marginBottom: 16 }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 14 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <span style={{ fontSize: 18 }}>📊</span>
            <h3 style={{ margin: 0, fontSize: 15 }}>Сводка сетевого трафика</h3>
            {trafficSummary.whitelist_active && (
              <span className="badge" style={{ background: 'rgba(168, 85, 247, 0.2)', color: '#c084fc', border: '1px solid rgba(168, 85, 247, 0.4)', fontSize: 11, fontWeight: 600 }}>
                🟣 БС активны
              </span>
            )}
          </div>
          <button
            type="button"
            className="btn-icon-subtle"
            title="Сбросить счетчики трафика всех направлений"
            onClick={() => handleResetTraffic('all', 'всего трафика')}
            style={{ fontSize: 12, padding: '4px 10px', background: 'rgba(255,255,255,0.04)', border: '1px solid var(--border)', borderRadius: 6, cursor: 'pointer', color: 'var(--text-muted)' }}
          >
            🔄 Сбросить статистику
          </button>
        </div>

        <p className="muted small" style={{ margin: '0 0 14px' }}>
          Накопленный объем трафика по направлениям маршрутизации. Сохраняется между перезагрузками роутера.
        </p>

        {/* 3 равных сбалансированных сегмента */}
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(220px, 1fr))', gap: 12, marginBottom: 12 }}>
          {/* 1. Прямой трафик */}
          <div style={{ background: 'rgba(255,255,255,0.02)', border: '1px solid var(--border)', borderRadius: 10, padding: 12, display: 'flex', flexDirection: 'column', justifyContent: 'space-between' }}>
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 6 }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 6, color: '#4ade80', fontSize: 13, fontWeight: 600 }}>
                <span>🟢</span>
                <span>Прямой трафик</span>
              </div>
              <button
                type="button"
                className="btn-icon-subtle"
                title="Сбросить счетчик прямого трафика"
                aria-label="Сбросить счетчик прямого трафика"
                onClick={() => handleResetTraffic('direct', 'прямого трафика')}
                style={{ fontSize: 13, padding: '2px 6px', background: 'rgba(255,255,255,0.04)', border: '1px solid var(--border)', borderRadius: 5, cursor: 'pointer', color: 'var(--text-muted)' }}
              >
                ↺
              </button>
            </div>
            <div style={{ fontSize: 22, fontWeight: 700, color: 'var(--text-bright)' }}>
              {fmtTrafficBytes(trafficSummary.direct_bytes)}
            </div>
            <div className="muted small" style={{ marginTop: 4, fontSize: 11 }}>
              Мимо прокси в WAN
            </div>
          </div>

          {/* 2. VPS трафик (Обычные серверы) */}
          <div style={{ background: 'rgba(255,255,255,0.02)', border: '1px solid var(--border)', borderRadius: 10, padding: 12, display: 'flex', flexDirection: 'column', justifyContent: 'space-between' }}>
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 6 }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 6, color: '#38bdf8', fontSize: 13, fontWeight: 600 }}>
                <span>🔵</span>
                <span>VPS трафик</span>
              </div>
              <button
                type="button"
                className="btn-icon-subtle"
                title="Сбросить счетчик VPS трафика"
                aria-label="Сбросить счетчик VPS трафика"
                onClick={() => handleResetTraffic('vps', 'VPS трафика')}
                style={{ fontSize: 13, padding: '2px 6px', background: 'rgba(255,255,255,0.04)', border: '1px solid var(--border)', borderRadius: 5, cursor: 'pointer', color: 'var(--text-muted)' }}
              >
                ↺
              </button>
            </div>
            <div style={{ fontSize: 22, fontWeight: 700, color: 'var(--text-bright)' }}>
              {fmtTrafficBytes(trafficSummary.proxy_bytes)}
            </div>
            <div className="muted small" style={{ marginTop: 4, fontSize: 11 }}>
              Обычные серверы
            </div>
          </div>

          {/* 3. Туннель белых списков */}
          <div style={{ background: trafficSummary.whitelist_active ? 'rgba(168, 85, 247, 0.08)' : 'rgba(255,255,255,0.02)', border: trafficSummary.whitelist_active ? '1px solid rgba(168, 85, 247, 0.4)' : '1px solid var(--border)', borderRadius: 10, padding: 12, display: 'flex', flexDirection: 'column', justifyContent: 'space-between' }}>
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 6 }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 6, color: '#c084fc', fontSize: 13, fontWeight: 600 }}>
                <span>🟣</span>
                <span>Туннель белых списков</span>
              </div>
              <button
                type="button"
                className="btn-icon-subtle"
                title="Сбросить счетчик трафика белых списков"
                aria-label="Сбросить счетчик трафика белых списков"
                onClick={() => handleResetTraffic('whitelist', 'трафика белых списков')}
                style={{ fontSize: 13, padding: '2px 6px', background: 'rgba(255,255,255,0.04)', border: '1px solid var(--border)', borderRadius: 5, cursor: 'pointer', color: 'var(--text-muted)' }}
              >
                ↺
              </button>
            </div>
            <div style={{ fontSize: 22, fontWeight: 700, color: 'var(--text-bright)' }}>
              {fmtTrafficBytes(trafficSummary.whitelist_bytes)}
            </div>
            <div className="muted small" style={{ marginTop: 4, fontSize: 11 }}>
              {trafficSummary.whitelist_active ? 'Активен для LAN устройств' : 'Серверы с меткой [БС]'}
            </div>
          </div>
        </div>

        {/* Квота подписки (во всю ширину карточки внизу) */}
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
            <div style={{ background: 'rgba(255,255,255,0.02)', border: '1px solid var(--border)', borderRadius: 10, padding: '10px 14px', fontSize: 11.5 }}>
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 6, flexWrap: 'wrap', gap: 6 }}>
                <span style={{ color: '#c084fc', fontWeight: 600 }}>
                  📊 Квота подписки ({activeWlProv.name || activeWlProv.id}): {formatQuotaBytes(used)} / {formatQuotaBytes(total)} ({percent}%)
                </span>
                <span style={{ color: remaining === 0 ? '#ef4444' : '#4ade80', fontWeight: 600 }}>
                  Осталось: {formatQuotaBytes(remaining)}
                </span>
              </div>
              <div style={{ width: '100%', height: 6, background: 'rgba(255,255,255,0.08)', borderRadius: 3, overflow: 'hidden' }}>
                <div
                  style={{
                    width: `${percent}%`,
                    height: '100%',
                    background: percent > 90 ? '#ef4444' : percent > 75 ? '#fbbf24' : 'linear-gradient(90deg, #a855f7, #c084fc)',
                    borderRadius: 3,
                  }}
                />
              </div>
              {expireStr && (
                <div className="muted" style={{ fontSize: 10.5, marginTop: 4 }}>
                  Действует до: {expireStr}
                </div>
              )}
            </div>
          )
        })()}
      </section>

      <div className="settings-columns">
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
                              title="Сервер отвечает на TCP-пинг, но блокирует реальный веб-трафик (L7-сбой передачи данных). Временно исключён из цепочки в карантине."
                            >
                              🧟 Зомби {quarantineInfo ? `(ещё ${Math.ceil(quarantineInfo.remaining_secs / 60)} мин)` : '(в карантине)'}
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
                              🛡️ Карантин ({quarantineInfo ? `ещё ${Math.ceil(quarantineInfo.remaining_secs / 60)} мин` : 'активен'})
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

            {/* Приоритет провайдеров в Failover (Tier 1 vs Tier 2) */}
            <div className="row" style={{ flexDirection: 'column', alignItems: 'stretch', gap: 8, background: 'rgba(255, 255, 255, 0.02)', padding: '12px 14px', borderRadius: 8, border: '1px solid var(--border)' }}>
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: 6 }}>
                <div>
                  <b style={{ display: 'block', fontSize: 13 }}>📡 Приоритет провайдеров подписок</b>
                  <span className="muted small">
                    Порядок переключения между поставщиками. Серверы Tier 1 (VPS) имеют преимущество над Tier 2 (белые списки).
                  </span>
                </div>
                {providerChain.length > 0 && (
                  <button
                    type="button"
                    className="btn sm ghost"
                    style={{ color: 'var(--muted)', fontSize: 11 }}
                    onClick={() => patch((s) => (s.failover.provider_chain = []))}
                    title="Сбросить порядок провайдеров на стандартный"
                  >
                    Сбросить
                  </button>
                )}
              </div>

              {providers.length === 0 ? (
                <span className="muted small">Провайдеры подписок не обнаружены.</span>
              ) : (
                <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                  {(() => {
                    // Combine providers: explicitly ordered ones first, then remaining
                    const ordered = [...providerChain]
                      .map((id) => providers.find((p) => p.id === id))
                      .filter((p): p is ProviderInfo => Boolean(p))
                    const remaining = providers.filter((p) => !providerChain.includes(p.id))
                    const allList = [...ordered, ...remaining]

                    return allList.map((p) => {
                      const inChain = providerChain.includes(p.id)
                      const chainIdx = providerChain.indexOf(p.id)
                      const isTier2 = Boolean(p.is_whitelist)

                      return (
                        <div
                          key={p.id}
                          className="provider-chain-row"
                          style={{
                            display: 'flex',
                            alignItems: 'center',
                            justifyContent: 'space-between',
                            gap: 10,
                            padding: '6px 10px',
                            background: inChain ? 'rgba(56, 189, 248, 0.04)' : 'rgba(255, 255, 255, 0.015)',
                            borderRadius: 6,
                            border: `1px solid ${inChain ? 'rgba(56, 189, 248, 0.25)' : 'rgba(255, 255, 255, 0.06)'}`,
                          }}
                        >
                          <div style={{ display: 'flex', alignItems: 'center', gap: 8, minWidth: 0, flex: 1 }}>
                            <input
                              type="checkbox"
                              checked={inChain}
                              onChange={() => toggleProviderInChain(p.id)}
                              title={inChain ? 'Исключить из приоритетной цепочки' : 'Включить в приоритетную цепочку'}
                              style={{ cursor: 'pointer', margin: 0 }}
                            />
                            {inChain && (
                              <span
                                style={{
                                  fontSize: 10,
                                  fontWeight: 700,
                                  padding: '1px 5px',
                                  borderRadius: 4,
                                  background: chainIdx === 0 ? 'rgba(56, 189, 248, 0.25)' : 'rgba(255, 255, 255, 0.08)',
                                  color: chainIdx === 0 ? '#38bdf8' : 'var(--muted)',
                                }}
                              >
                                #{chainIdx + 1}
                              </span>
                            )}
                            <span style={{ fontWeight: 600, fontSize: 12.5, color: inChain ? 'var(--text-bright)' : 'var(--muted)' }}>
                              {p.name || p.id}
                            </span>
                            {/* Tier индикатор: Tier 1 (VPS) или Tier 2 (БС) */}
                            {isTier2 ? (
                              <span
                                className="badge"
                                style={{
                                  fontSize: 9.5,
                                  padding: '1px 6px',
                                  background: 'rgba(234, 179, 8, 0.15)',
                                  color: '#eab308',
                                  border: '1px solid rgba(234, 179, 8, 0.3)',
                                  fontWeight: 600,
                                }}
                              >
                                🛡️ Tier 2 (БС)
                              </span>
                            ) : (
                              <span
                                className="badge"
                                style={{
                                  fontSize: 9.5,
                                  padding: '1px 6px',
                                  background: 'rgba(56, 189, 248, 0.15)',
                                  color: '#38bdf8',
                                  border: '1px solid rgba(56, 189, 248, 0.3)',
                                  fontWeight: 600,
                                }}
                              >
                                ⚡ Tier 1 (VPS)
                              </span>
                            )}
                            <span className="muted" style={{ fontSize: 11 }}>({p.count} узлов)</span>
                          </div>

                          {inChain && (
                            <div style={{ display: 'flex', alignItems: 'center', gap: 4, flexShrink: 0 }}>
                              <button
                                type="button"
                                className="btn sm ghost"
                                style={{ height: 22, width: 22, padding: 0, fontSize: 10, display: 'inline-flex', alignItems: 'center', justifyContent: 'center' }}
                                disabled={chainIdx === 0}
                                onClick={() => moveProviderChain(chainIdx, -1)}
                                title="Повысить приоритет провайдера"
                              >
                                ▲
                              </button>
                              <button
                                type="button"
                                className="btn sm ghost"
                                style={{ height: 22, width: 22, padding: 0, fontSize: 10, display: 'inline-flex', alignItems: 'center', justifyContent: 'center' }}
                                disabled={chainIdx === providerChain.length - 1}
                                onClick={() => moveProviderChain(chainIdx, 1)}
                                title="Понизить приоритет провайдера"
                              >
                                ▼
                              </button>
                            </div>
                          )}
                        </div>
                      )
                    })
                  })()}
                </div>
              )}
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

            {/* 🧠 Умный фейловер (приоритет VPS над белыми списками) */}
            <div className="row" style={{ justifyContent: 'space-between', alignItems: 'center' }}>
              <div>
                <b style={{ display: 'block', fontSize: 13 }}>🧠 Умный фейловер (приоритет VPS над белыми списками)</b>
                <span className="muted small">Автоматически возвращает трафик на VPS, когда они оживают, защищая лимитированные узлы [БС]</span>
              </div>
              <div style={{ display: 'inline-flex', alignItems: 'center', gap: 10 }}>
                <label className="switch" title="Включить или выключить умный фейловер">
                  <input
                    type="checkbox"
                    checked={settings.failover.smart_failover_enabled ?? true}
                    onChange={(e) => patch((s) => (s.failover.smart_failover_enabled = e.target.checked))}
                    data-testid="settings-smart-failover-toggle"
                  />
                  <span className="slider" />
                </label>
                <span style={{ fontSize: 13, minWidth: 64, color: (settings.failover.smart_failover_enabled ?? true) ? 'var(--green)' : 'var(--muted)' }}>
                  {(settings.failover.smart_failover_enabled ?? true) ? 'Включён' : 'Выключен'}
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

            {/* Whitelist Detector Card */}
            <WhitelistDetectorSection
              settings={settings}
              servers={servers}
              providers={providers}
              patch={patch}
              notify={notify}
              refresh={refresh}
            />

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

