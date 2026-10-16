import { useCallback, useEffect, useRef, useState } from 'react'
import { apiGet, apiPost, apiPut } from '../api'
import {
  pingClass,
  type AppSettings,
  type DnsMode,
  type DnsEnhancedMode,
  type ServerInfo,
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

interface Props {
  notify: (msg: string, isError?: boolean) => void
  status?: StatusInfo | null
  refresh?: () => void
}

export default function Settings({ notify, status, refresh }: Props) {
  const [settings, setSettings] = useState<AppSettings | null>(null)
  const [servers, setServers] = useState<ServerInfo[]>([])
  const [saving, setSaving] = useState(false)
  const [directDomains, setDirectDomains] = useState('')
  const [forceDomains, setForceDomains] = useState('')
  const [autoCdns, setAutoCdns] = useState<string[]>([])
  const [cdnsCollapsed, setCdnsCollapsed] = useState(true)
  const [savingDomains, setSavingDomains] = useState(false)
  const [scanningCdn, setScanningCdn] = useState(false)
  const [domainsRevision, setDomainsRevision] = useState<string | undefined>(undefined)

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

  // --- Zapret ---
  interface ZapretStatus {
    installed: boolean
    running: boolean
    pid?: number | null
  }
  const [zapretStatus, setZapretStatus] = useState<ZapretStatus | null>(null)

  const loadQuickWins = useCallback(async () => {
    try {
      const adb = await apiGet<{ enabled: boolean }>('adblock')
      setAdblockEnabled(adb.enabled)
    } catch {}
    try {
      const geo = await apiGet<GeoInfo>('system/geo-info')
      setGeoInfo(geo)
    } catch {}
    try {
      const zap = await apiGet<ZapretStatus>('zapret/status')
      setZapretStatus(zap)
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

            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(200px, 1fr))', gap: 10 }}>
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
            </div>

            <div className="row" style={{ flexDirection: 'column', alignItems: 'stretch', gap: 6 }}>
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                <span style={{ fontWeight: 600, fontSize: 13 }}>Цепочка приоритетов серверов и пулов</span>
                <span className="muted small">{chain.length} узлов в цепочке</span>
              </div>

              <div className="modal-list" style={{ maxHeight: 280, display: 'flex', flexDirection: 'column', gap: 4 }}>
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
                  return (
                    <div
                      key={id}
                      className="check-row"
                      style={{
                        display: 'flex',
                        gap: 8,
                        alignItems: 'center',
                        background: i === 0 ? 'rgba(56, 189, 248, 0.08)' : 'rgba(255, 255, 255, 0.02)',
                        border: i === 0 ? '1px solid rgba(56, 189, 248, 0.3)' : '1px solid var(--border)',
                        borderRadius: 8,
                        padding: '6px 10px',
                      }}
                    >
                      <span
                        style={{
                          fontSize: 10.5,
                          fontWeight: 700,
                          padding: '2px 6px',
                          borderRadius: 4,
                          background: i === 0 ? 'rgba(56, 189, 248, 0.25)' : 'rgba(255, 255, 255, 0.08)',
                          color: i === 0 ? '#38bdf8' : 'var(--muted)',
                        }}
                      >
                        {i === 0 ? '👑 ОСНОВНОЙ' : `⚡ РЕЗЕРВ ${i}`}
                      </span>

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

                      <span className="server-name" style={{ flex: 1, fontWeight: i === 0 ? 600 : 400 }} title={id}>
                        {sv ? sv.name : `${id} (недоступен)`}
                      </span>

                      {(sv || livePing > 0) && (
                        <span className={'ping ' + pingClass(livePing)}>
                          {livePing > 0 ? `${livePing} мс` : '—'}
                        </span>
                      )}

                      <button className="btn sm ghost" disabled={i === 0} onClick={() => chainMove(i, -1)} title="Повысить приоритет">
                        ↑
                      </button>
                      <button className="btn sm ghost" disabled={i === chain.length - 1} onClick={() => chainMove(i, 1)} title="Понизить приоритет">
                        ↓
                      </button>
                      <button
                        className="btn sm ghost"
                        style={{ color: '#ef4444' }}
                        onClick={() => patch((s) => (s.failover.priority_chain = chain.filter((x) => x !== id)))}
                        title="Удалить из цепочки"
                      >
                        ✕
                      </button>
                    </div>
                  )
                })}

                <select
                  className="select"
                  value=""
                  onChange={(e) => {
                    if (!e.target.value) return
                    if (!chain.includes(e.target.value)) {
                      patch((s) => (s.failover.priority_chain = [...chain, e.target.value]))
                    }
                  }}
                  style={{ marginTop: 4 }}
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
                        return (
                          <option key={s.id} value={s.id}>
                            {s.provider_name ? `[${s.provider_name}] ` : s.provider ? `[${s.provider}] ` : ''}{s.name} · {optPing > 0 ? `${optPing} мс` : '—'}
                          </option>
                        )
                      })}
                  </optgroup>
                </select>
              </div>

              {chain.length > 0 && (
                <div style={{ display: 'flex', justifyContent: 'flex-end', marginTop: 4 }}>
                  <button className="btn sm ghost" onClick={() => patch((s) => (s.failover.priority_chain = []))}>
                    Очистить цепочку
                  </button>
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

          {/* СТОРОЖЕВОЙ ТАЙМЕР ЯДРА (WATCHDOG & AUTO-HEALING) */}
          <section className="card">
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 8 }}>
              <h2 style={{ margin: 0 }}>🛡️ Сторожевой таймер ядра (Watchdog)</h2>
              <span className="badge badge-online" style={{ color: '#22c55e' }}>
                🟢 Активен (авто-лечение)
              </span>
            </div>
            <p className="muted small">
              Фоновый сторожевой процесс демона непрерывно контролирует целостность правил маршрутизации в <code>config.yaml</code>.
            </p>
            <div className="stats-grid four-col" style={{ marginTop: 12 }}>
              <div className="stat-card">
                <div className="stat-label">Статус сторожа</div>
                <div className="stat-value" style={{ fontSize: 14, color: '#22c55e' }}>
                  ✓ Авто-лечение
                </div>
                <div className="muted small">Контроль config.yaml</div>
              </div>
              <div className="stat-card">
                <div className="stat-label">Интервал проверки</div>
                <div className="stat-value" style={{ fontSize: 18 }}>
                  4 сек
                </div>
                <div className="muted small">Фоновый демон</div>
              </div>
              <div className="stat-card">
                <div className="stat-label">Защита блоков</div>
                <div className="stat-value" style={{ fontSize: 13, color: 'var(--accent)', fontFamily: 'monospace' }}>
                  DEVICE, FORCE, IGNORE
                </div>
                <div className="muted small">Авто-восстановление</div>
              </div>
              <div className="stat-card">
                <div className="stat-label">При рестарте XKeen</div>
                <div className="stat-value" style={{ fontSize: 13, color: '#38bdf8' }}>
                  Без разрыва связи
                </div>
                <div className="muted small">Бесшовный накат правил</div>
              </div>
            </div>
          </section>

          {/* ZAPRET / DPI */}
          <section className="card">
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 8 }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                <span style={{ fontSize: 20 }}>⚡</span>
                <h2 style={{ margin: 0 }}>Zapret — Обход замедлений DPI</h2>
              </div>
              <span className="badge" style={{ color: zapretStatus?.running ? '#22c55e' : zapretStatus?.installed ? '#f59e0b' : 'var(--muted)' }}>
                {zapretStatus?.running ? `🟢 Активен (PID: ${zapretStatus.pid})` : zapretStatus?.installed ? '⚪ Выключен' : '🔴 Не установлен'}
              </span>
            </div>
            <p className="muted small">
              Все параметры обхода DPI (мульти-стратегии для YouTube, Discord, списки доменов и онлайн-тест) сосредоточены во вкладке <b>«⚡ Zapret (DPI)»</b>.
            </p>
            <div style={{ display: 'flex', gap: 10, marginTop: 12 }}>
              <button
                type="button"
                className="btn primary sm"
                onClick={() => window.dispatchEvent(new CustomEvent('xr:switch-tab', { detail: 'zapret' }))}
              >
                ⚡ Перейти в настройки Zapret →
              </button>
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

      {/* БЕЗОПАСНОСТЬ И ПАРОЛЬ (В САМЫЙ КОНЕЦ НАСТРОЕК) */}
      <section className="card">
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 8 }}>
          <h2 style={{ margin: 0 }}>🔐 Безопасность и пароль</h2>
          <span className={`badge ${authEnabled ? 'badge-online' : ''}`}>
            {authEnabled ? '🟢 защита включена' : '⚪ без пароля'}
          </span>
        </div>
        <p className="muted small">Защита веб-панели паролем с сессией (24 часа).</p>
        <div className="row" style={{ justifyContent: 'space-between', alignItems: 'center' }}>
          <b>Включить защиту паролем</b>
          <label className="switch">
            <input
              type="checkbox"
              checked={authEnabled}
              onChange={(e) => setAuthEnabled(e.target.checked)}
            />
            <span className="slider" />
          </label>
        </div>

        {authEnabled && (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 8, marginTop: 8 }}>
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
        )}

        <button className="btn primary" style={{ marginTop: 10 }} onClick={handleSaveAuth} disabled={authSaving}>
          {authSaving ? 'Сохранение…' : '💾 Сохранить настройки доступа'}
        </button>
        <p className="muted small" style={{ marginTop: 8 }}>
          💡 Сброс пароля при утере через SSH: <code style={{ color: 'var(--accent)' }}>xkeen-route reset-password</code>
        </p>
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

