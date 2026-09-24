import { useCallback, useEffect, useRef, useState } from 'react'
import { apiGet, apiPost, apiPut } from '../api'
import { pingClass, type AppSettings, type DnsMode, type DnsEnhancedMode, type ServerInfo, type StatusInfo } from '../types'
import NumberInput from './NumberInput'
import ConfigEditor from './ConfigEditor'
import PresetCatalogModal from './PresetCatalogModal'
import LogsViewer from './LogsViewer'

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
  const [savingDomains, setSavingDomains] = useState(false)

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
  // --- Обновление панели ---
  interface UpdateInfo { current: string; latest: string; update_available: boolean; notes: string[] }
  const [upd, setUpd] = useState<UpdateInfo | null>(null)
  const [updBusy, setUpdBusy] = useState(false)
  const [updStage, setUpdStage] = useState('')
  const [updError, setUpdError] = useState('')
  const [updChecking, setUpdChecking] = useState(false)
  const [showUpdNotes, setShowUpdNotes] = useState(false)

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
  const [zapretBusy, setZapretBusy] = useState(false)

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

  const handleZapretAction = async (action: 'start' | 'stop' | 'restart' | 'install') => {
    setZapretBusy(true)
    try {
      await apiPost('zapret/action', { action })
      notify(action === 'install' ? 'Установка Zapret завершена' : `Команда Zapret '${action}' выполнена`)
      try {
        const zap = await apiGet<ZapretStatus>('zapret/status')
        setZapretStatus(zap)
      } catch {}
      loadQuickWins()
    } catch (e: any) {
      notify('Ошибка Zapret: ' + e.message, true)
    } finally {
      setZapretBusy(false)
    }
  }

  useEffect(() => {
    loadQuickWins()
    apiGet<AppSettings>('settings').then((s) => {
      setSettings(s)
      setAuthEnabled(s.auth?.enabled ?? false)
      setTelegramEnabled(s.notifications?.telegram_enabled ?? false)
      setTelegramBotToken(s.notifications?.telegram_bot_token ?? '')
      setTelegramChatId(s.notifications?.telegram_chat_id ?? '')
      setWebhookUrl(s.notifications?.webhook_url ?? '')
    }).catch((e) => notify(e instanceof Error ? e.message : 'Ошибка', true))

    apiGet<{ servers: ServerInfo[] }>('servers')
      .then((d) => setServers(d.servers))
      .catch(() => {})
    apiGet<{ direct: string[]; force: string[]; auto_cdns?: string[] }>('domains')
      .then((d) => {
        setDirectDomains(d.direct.join('\n'))
        setForceDomains(d.force.join('\n'))
        if (d.auto_cdns) setAutoCdns(d.auto_cdns)
      })
      .catch(() => {})
    apiGet<DnsMode>('dns/mode')
      .then((d) => setDnsMode(d.enhanced_mode))
      .catch(() => {})
  }, [notify])

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

  // Проверка новой версии панели.
  const checkUpdate = useCallback(async () => {
    setUpdChecking(true)
    setUpdError('')
    try {
      const data = await apiGet<UpdateInfo>('update/check')
      setUpd(data)
    } catch (e) {
      setUpdError(e instanceof Error ? e.message : 'Ошибка проверки')
    } finally {
      setUpdChecking(false)
    }
  }, [])

  useEffect(() => {
    checkUpdate()
  }, [checkUpdate])

  const doUpdate = async () => {
    if (!confirm(`Установить обновление ${upd?.latest}? Панель перезапустится автоматически.`)) return
    setUpdBusy(true)
    setUpdStage('Загрузка и установка…')
    try {
      await apiPost<{ installed: string; restarting: boolean }>('update/install')
      setUpdStage(`Установлена ${upd?.latest}. Перезапуск панели…`)
      setTimeout(() => location.reload(), 8000)
    } catch (e) {
      setUpdBusy(false)
      setUpdStage('')
      notify(e instanceof Error ? e.message : 'Ошибка обновления', true)
    }
  }

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
        await apiPost<{ message?: string }>('settings/priority', {
          server_ids: currentFailover.priority_chain ?? [],
          enabled: currentFailover.enabled,
        })
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
      await apiPut('settings', settings)
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
      }>('domains', {
        direct: directDomains.split('\n'),
        force: forceDomains.split('\n'),
      })
      if (data.auto_cdns) setAutoCdns(data.auto_cdns)
      const cdnMsg = data.auto_cdns && data.auto_cdns.length > 0 ? `, CDN: +${data.auto_cdns.length}` : ''
      const ipMsg = data.overridden_ips ? `, IP: ${data.overridden_ips}` : ''
      notify(`Домены сохранены: напрямую ${data.direct}, через прокси ${data.force}${cdnMsg}${ipMsg}`)
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка сохранения доменов', true)
    } finally {
      setSavingDomains(false)
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
      const formData = new FormData()
      formData.append('file', file)
      const res = await fetch('/api/backups/import', { method: 'POST', body: formData })
      const data = await res.json()
      if (!data.success) throw new Error(data.error || 'Ошибка импорта')
      notify(`Бэкап '${file.name}' успешно импортирован`)
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
    setNotifTesting(true)
    try {
      const res = await apiPost<{ message: string }>('notifications/test', {
        telegram_bot_token: telegramBotToken.trim() || undefined,
        telegram_chat_id: telegramChatId.trim() || undefined,
        webhook_url: webhookUrl.trim() || undefined,
      })
      notify(res.message || 'Тестовое оповещение успешно отправлено')
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
        </div>
      </div>

      <div className="settings-columns">
        {/* ЛЕВАЯ КОЛОНКА */}
        <div className="settings-col">
          {/* FAILOVER КАРТОЧКА */}
          <section className="card">
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 12 }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                <h2 style={{ margin: 0 }}>Failover</h2>
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
            <label className="row"><span>Порог пинга, мс</span>
              <NumberInput
                min={50}
                max={5000}
                step={50}
                fallback={300}
                value={settings.failover.ping_threshold_ms}
                onChange={(val) => patch((s) => (s.failover.ping_threshold_ms = val))}
              />
            </label>
            <div className="row" style={{ flexDirection: 'column', alignItems: 'stretch', gap: 6 }}>
              <span>Цепочка приоритетов (первый — основной)</span>
              <div className="modal-list" style={{ maxHeight: 260 }}>
                {chain.length === 0 && <p className="muted small" style={{ margin: 0 }}>Цепочка не задана — failover выбирает лучший доступный сервер.</p>}
                {chain.map((id, i) => {
                  const sv = servers.find((x) => x.id === id)
                  return (
                    <div key={id} className="check-row" style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
                      <span className="badge">{i === 0 ? 'ОСН' : `РЕЗ${i}`}</span>
                      {sv?.provider && (
                        <span className="tag-provider" style={{ fontSize: 10, padding: '1px 5px', cursor: 'default' }}>
                          📦 {sv.provider_name || sv.provider}
                        </span>
                      )}
                      <span className="server-name" style={{ flex: 1 }} title={id}>{sv ? sv.name : `${id} (сейчас недоступен)`}</span>
                      {sv && (
                        <span className={'ping ' + pingClass(sv.ping_ms)}>
                          {sv.ping_ms > 0 ? `${sv.ping_ms} мс` : '—'}
                        </span>
                      )}
                      <button className="btn sm ghost" disabled={i === 0} onClick={() => chainMove(i, -1)}>↑</button>
                      <button className="btn sm ghost" disabled={i === chain.length - 1} onClick={() => chainMove(i, 1)}>↓</button>
                      <button
                        className="btn sm ghost"
                        onClick={() => patch((s) => (s.failover.priority_chain = chain.filter((x) => x !== id)))}
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
                >
                  <option value="">+ добавить сервер в цепочку…</option>
                  {servers
                    .filter((s) => !chain.includes(s.id))
                    .map((s) => (
                      <option key={s.id} value={s.id}>
                        {s.provider_name ? `[${s.provider_name}] ` : s.provider ? `[${s.provider}] ` : ''}{s.name} · {s.ping_ms > 0 ? `${s.ping_ms} мс` : '—'}
                      </option>
                    ))}
                </select>
              </div>
              {chain.length > 0 && (
                <div style={{ display: 'flex', justifyContent: 'flex-end', marginTop: 6 }}>
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
            <label className="row"><span>Интервал проверки, сек</span>
              <NumberInput
                min={15}
                max={3600}
                step={5}
                fallback={60}
                value={settings.failover.interval_secs}
                onChange={(val) => patch((s) => (s.failover.interval_secs = val))}
              />
            </label>
            <div style={{ display: 'flex', gap: 8, justifyContent: 'space-between', alignItems: 'center', marginTop: 12, flexWrap: 'wrap' }}>
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
          <section className="card">
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 6 }}>
              <h2 style={{ margin: 0 }}>🌐 Домены</h2>
              <button type="button" className="btn sm" onClick={() => setPresetCatalogOpen(true)} title="Добавить готовые списки (YouTube, Discord, AI...)">
                ✨ Каталог пресетов
              </button>
            </div>
            <p className="muted small">По одному домену в строке. Правила вставляются в начало rules: (DOMAIN-SUFFIX) и имеют приоритет. Сопутствующие CDN и медиа-сервера подтягиваются автоматически.</p>
            <label className="row" style={{ flexDirection: 'column', alignItems: 'stretch' }}>
              <span>⏭ Напрямую (мимо прокси → DIRECT)</span>
              <textarea
                className="input"
                rows={6}
                placeholder={'example.com\nlocal-service.net\nw3.org'}
                value={directDomains}
                onChange={(e) => setDirectDomains(e.target.value)}
                style={{ fontFamily: 'Consolas, monospace', fontSize: 12.5, resize: 'vertical' }}
              />
            </label>
            <label className="row" style={{ flexDirection: 'column', alignItems: 'stretch', marginTop: 8 }}>
              <span>🔒 Принудительно через прокси (→ PROXY)</span>
              <textarea
                className="input"
                rows={6}
                placeholder={'openai.com\nyoutube.com\ngithub.com'}
                value={forceDomains}
                onChange={(e) => setForceDomains(e.target.value)}
                style={{ width: '100%', boxSizing: 'border-box', fontFamily: 'Consolas, monospace', fontSize: 12.5, resize: 'vertical' }}
              />
            </label>
            {autoCdns.length > 0 && (
              <div style={{ marginTop: 8, padding: '8px 10px', background: 'rgba(56, 189, 248, 0.08)', border: '1px solid rgba(56, 189, 248, 0.2)', borderRadius: 6, fontSize: 12 }}>
                <div style={{ color: '#38bdf8', fontWeight: 600, marginBottom: 4 }}>
                  ⚡ Автоматически подключенные CDN и медиа-сервера ({autoCdns.length}):
                </div>
                <div style={{ display: 'flex', gap: 4, flexWrap: 'wrap' }}>
                  {autoCdns.map((cdn) => (
                    <span key={cdn} style={{ fontSize: 11, background: 'rgba(56, 189, 248, 0.15)', color: '#38bdf8', padding: '1px 6px', borderRadius: 4, fontFamily: 'monospace' }}>
                      {cdn}
                    </span>
                  ))}
                </div>
              </div>
            )}
            <button className="btn primary" style={{ marginTop: 12 }} onClick={saveDomains} disabled={savingDomains}>
              {savingDomains ? 'Применение и поиск CDN…' : '🌐 Применить домены'}
            </button>
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
            <label className="row">
              <span>Bot Token</span>
              <input
                className="input"
                placeholder="123456789:ABCdefGhIJKlmNoPQRstuVWXyz"
                value={telegramBotToken}
                onChange={(e) => setTelegramBotToken(e.target.value)}
              />
            </label>
            <label className="row">
              <span>Chat ID</span>
              <input
                className="input"
                placeholder="123456789 или -1001234567890"
                value={telegramChatId}
                onChange={(e) => setTelegramChatId(e.target.value)}
              />
            </label>
            <label className="row">
              <span>Webhook URL</span>
              <input
                className="input"
                placeholder="https://my-server.com/api/failover-hook"
                value={webhookUrl}
                onChange={(e) => setWebhookUrl(e.target.value)}
              />
            </label>
            <div style={{ display: 'flex', gap: 8, marginTop: 10, flexWrap: 'wrap' }}>
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
              <h2 style={{ margin: 0 }}>⚡ Обход замедлений Zapret (nfqws)</h2>
              <span className="badge" style={{ color: zapretStatus?.running ? '#22c55e' : zapretStatus?.installed ? '#f59e0b' : 'var(--muted)' }}>
                {zapretStatus?.running ? `🟢 Запущен (PID: ${zapretStatus.pid})` : zapretStatus?.installed ? '🟡 Остановлен' : '⚪ Не установлен'}
              </span>
            </div>
            <p className="muted small">
              Локальный сервис для обхода DPI-замедлений YouTube, Discord и других сервисов без расхода трафика VPS (/opt/etc/init.d/S51zapret).
            </p>
            <div className="stats-grid four-col" style={{ marginTop: 10, marginBottom: 12 }}>
              <div className="stat-card">
                <div className="stat-label">Статус службы</div>
                <div className="stat-value" style={{ fontSize: 14, color: zapretStatus?.running ? '#22c55e' : zapretStatus?.installed ? '#f59e0b' : 'var(--muted)' }}>
                  {zapretStatus?.running ? '🟢 Запущен' : zapretStatus?.installed ? '🟡 Остановлен' : '⚪ Не установлен'}
                </div>
                <div className="muted small">{zapretStatus?.running ? `PID: ${zapretStatus.pid}` : 'Служба DPI'}</div>
              </div>
              <div className="stat-card">
                <div className="stat-label">Исполняемый демон</div>
                <div className="stat-value" style={{ fontSize: 14, fontFamily: 'monospace' }}>
                  nfqws
                </div>
                <div className="muted small">S51zapret</div>
              </div>
              <div className="stat-card">
                <div className="stat-label">Расход VPS</div>
                <div className="stat-value" style={{ fontSize: 14, color: '#10b981' }}>
                  0 байт (Direct)
                </div>
                <div className="muted small">Прямой поток роутера</div>
              </div>
              <div className="stat-card">
                <div className="stat-label">Назначение</div>
                <div className="stat-value" style={{ fontSize: 13, color: 'var(--accent)' }}>
                  YouTube, Discord
                </div>
                <div className="muted small">Обход замедлений</div>
              </div>
            </div>
            {zapretStatus?.installed ? (
              <div style={{ display: 'flex', gap: 8, marginTop: 10, flexWrap: 'wrap' }}>
                <button
                  type="button"
                  className="btn sm"
                  disabled={zapretBusy}
                  onClick={() => handleZapretAction(zapretStatus.running ? 'restart' : 'start')}
                >
                  {zapretBusy ? '⏳…' : zapretStatus.running ? '🔄 Перезапустить' : '▶ Запустить'}
                </button>
                {zapretStatus.running && (
                  <button
                    type="button"
                    className="btn sm btn-danger"
                    disabled={zapretBusy}
                    onClick={() => handleZapretAction('stop')}
                  >
                    ⏹ Остановить
                  </button>
                )}
              </div>
            ) : (
              <div style={{ marginTop: 10 }}>
                <p className="muted small">
                  Пакет Zapret не обнаружен в /opt/etc/init.d/S51zapret. Вы можете установить его в один клик.
                </p>
                <button
                  type="button"
                  className="btn sm btn-primary"
                  disabled={zapretBusy}
                  onClick={() => handleZapretAction('install')}
                  style={{ marginTop: 8 }}
                >
                  {zapretBusy ? '⏳ Установка Zapret…' : '📥 Установить Zapret в 1 клик'}
                </button>
              </div>
            )}
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
                  {geoInfo ? `${(geoInfo.geoip.size / (1024 * 1024)).toFixed(1)} МБ` : '—'}
                </div>
                <div className="muted small">{geoInfo?.geoip.updated_at || 'Mihomo Core'}</div>
              </div>
              <div className="stat-card">
                <div className="stat-label">База GeoSite</div>
                <div className="stat-value" style={{ fontSize: 16 }}>
                  {geoInfo ? `${(geoInfo.geosite.size / (1024 * 1024)).toFixed(1)} МБ` : '—'}
                </div>
                <div className="muted small">{geoInfo?.geosite.updated_at || 'Meta Rules Dat'}</div>
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
              <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
                <span className="muted small">
                  Версия панели: <b>{upd?.current || status?.version || '…'}</b>
                </span>

                {updChecking && <span className="muted small">⏳ Проверка…</span>}

                {!updChecking && upd && !upd.update_available && (
                  <span className="muted small">✓ актуальная</span>
                )}

                {!updChecking && upd?.update_available && (
                  <div style={{ display: 'inline-flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
                    <span className="upd-dot" title="Доступна новая версия" />
                    <button
                      type="button"
                      className="btn btn-primary upd-glow"
                      disabled={updBusy}
                      onClick={doUpdate}
                      title={`Установить обновление ${upd.latest} в один клик`}
                      style={{ fontWeight: 600, padding: '5px 12px' }}
                    >
                      {updBusy ? '⏳ Установка…' : `🚀 Установить ${upd.latest}`}
                    </button>
                    {upd.notes.length > 0 && (
                      <button
                        type="button"
                        className="btn sm ghost"
                        disabled={updBusy}
                        onClick={() => setShowUpdNotes((prev) => !prev)}
                      >
                        {showUpdNotes ? 'Скрыть список изменений' : 'Что нового'}
                      </button>
                    )}
                  </div>
                )}

                {!updChecking && updError && (
                  <span className="small" style={{ color: 'var(--red, #e53935)' }}>
                    ⚠ {updError}
                  </span>
                )}

                <button
                  className="btn sm ghost"
                  disabled={updChecking || updBusy}
                  onClick={checkUpdate}
                  title="Проверить наличие обновлений"
                >
                  🔄 Проверить
                </button>
              </div>

              {upd?.update_available && upd.notes.length > 0 && showUpdNotes && (
                <div
                  style={{
                    margin: '10px 0 0',
                    padding: '10px 14px',
                    background: 'rgba(0, 0, 0, 0.2)',
                    borderRadius: 8,
                    border: '1px solid var(--border)',
                  }}
                >
                  <p className="small" style={{ margin: '0 0 6px', fontWeight: 600 }}>
                    Что нового в {upd.latest}:
                  </p>
                  <ul className="small" style={{ margin: 0, paddingLeft: 18, lineHeight: 1.5 }}>
                    {upd.notes.map((n, i) => (
                      <li key={i} style={{ marginBottom: 4 }}>
                        {n}
                      </li>
                    ))}
                  </ul>
                </div>
              )}
              {updStage && (
                <div
                  style={{
                    margin: '10px 0 0',
                    padding: '8px 12px',
                    background: 'rgba(34, 197, 94, 0.12)',
                    border: '1px solid rgba(34, 197, 94, 0.3)',
                    borderRadius: 6,
                    color: '#22c55e',
                    fontWeight: 600,
                    fontSize: 13,
                  }}
                >
                  ⏳ {updStage}
                </div>
              )}
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
        <p className="muted small">Защита веб-панели паролем с постоянной сессией (30 дней).</p>
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

